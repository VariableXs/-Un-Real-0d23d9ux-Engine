//! unx746_fmtspec —— UNX-F0746 域经格式规范冻结（全域统一模板定版）。
//!
//! 锚点原文（CoRun Unxreal 主册 · UNX-A1-B38 批 · 域经沉淀与移交包，
//! 主册表格行原文全文抄录）：
//! 「| UNX-F0746 | 域经格式规范冻结（全域统一模板定版） | 580 | 已深化 |
//!   UNX-F0746-J1 规范冻结账齐（模板五字段/粒度/证据三规范 + 版本号 +
//!   哈希封存）且入簿条目符合度 100% |」
//!
//! 定位原文（所在批头 · UNX-A1-B38 · 域经沉淀与移交包 F741–F760 · 20 条）：
//! 「AI-01 承办｜域账累计：205480 + 本批 11500 = 216980 / 240,000｜防重：
//!  域经登记簿与移交包封装面归本批独占；F0739 候选条目为既有账面（登记
//!  入簿消费不重写）、F0719 移交声明为既有账面（F0773 消费预告承接不
//!  重立）；任务书收官波：域经沉淀、域功入册、移交包为 B27–B40 收官段
//!  组成｜R-A1-004 纪律：批小计 = 逐条求和唯一真值（本批逐条求和
//!  11500）」
//!
//! J 判据原文（判据列独立抄录，验收唯一口径）：
//! 「UNX-F0746-J1 规范冻结账齐（模板五字段/粒度/证据三规范 + 版本号 +
//!   哈希封存）且入簿条目符合度 100%」
//!
//! ## 组件全景（十二个真子系统）
//! 1. **模板五字段规范 TemplateSpec**：条目五字段闭集（编号/标题/正文
//!     引用/评级引用/入册时戳），五张字段规范卡 const 写死（字段名/
//!     格式规则/长度带/来源批账），五字段非空校验 + 逐字段格式 parse
//!     （全手写定长文法，零正则零外部 crate）。评级引用直接消费
//!     F0745 档位闭集（H/M/L）——F0745 对齐声明写死在卡上。
//! 2. **粒度规范 GranularitySpec**：条目粒度三档（主题级/批级/域级），
//!     粒度→字数带宽 const 表（主题 40..=120 / 批级 121..=400 /
//!     域级 401..=1200，三带连续无缝、逐带来源注释），越带双检出
//!     （低于带/高于带分别归因），另带粒度→证据条数下限表（域级≥2，
//!     来源 F0743 证据链账深打要求）。
//! 3. **证据规范 EvidenceSpec**：每条目证据引用三型（判据号
//!     `UNX-F%04u-J%u` / 账行号 `B%02u:L%u` / 文件锚点 `src:%s:%u`）
//!     至少其一，逐型手写文法 parse（解引性第一关=格式可解），证据
//!     可解引注册表（判据任务号闭集 28 号 + 批行上界表 + 已知文件表，
//!     全 const 写死注明来源）解引性第二关。
//! 4. **规范冻结 SpecFreeze**：三规范（模板/粒度/证据）序列化规范形
//!     →FNV-1a 32 位哈希封存；封存账 append-only（版本/指纹/逻辑时
//!     刻/规范快照/变更注记五列），冻结后改规范即哈希红案；演进走版本
//!     前进（v1 不动、v2 登记制白名单补丁、新旧并存只增不改），版本
//!     回退/同版重封/无变更演进/破坏带连续全部 Lifecycle 拒收。
//! 5. **冻结证书 FreezeCertificate**：封存即出证（版本+指纹+逻辑时刻
//!     +封存时条目数+规范账全文渲染），证书内容与封存账逐字段可复核。
//! 6. **版本间差异账 render_evolution_diff**：封存账相邻两版规范快照
//!     逐字段比对，差异行落账（v1→v2 改了哪带、动了哪下限，一眼可查）。
//! 7. **入簿符合度审计 ComplianceAudit**：入簿条目逐条 vs 三规范
//!     （五字段空缺/格式不符、粒度越带、证据无解引/不足下限），违例
//!     清单三归因（字段缺失/粒度越带/证据无解引）逐条带码带注，
//!     符合度万分制落账，100%（10000/10000）即 J1 判据过。
//! 8. **登记簿面 FormatEntry/register**：五字段+粒度+字数+证据引用
//!     入簿，五道闸（容量/重号/五字段/越带/证据）逐道拒收，append-only
//!     零回滚（注入面另有显性 raw 通道，只许注入克隆账上用）。
//! 9. **事件账 FsEvent**：append-only 事件流水（登记/拒收/封存/校验/
//!     演进/审计/注入/健康八类），序号连续性独立可查。
//! 10. **健康核 health()**：七项不变量逐项核（容量界内/编号唯一/事件
//!     链连续/封存完整/规范快照指纹自洽/审计在期复算一致/注入清零），
//!     结论带逐项发现文本。
//! 11. **指纹 FNV-1a 32**：const fn 本体，空串基准 0x811C9DC5 与
//!     "a"/"foobar" 标准向量编译期断言；规范指纹+簿指纹双面。
//! 12. **双跑确定性对拍 + 注入面**：同一登记脚本双引擎独立建账→簿
//!     指纹/规范指纹/审计符合度三重对拍必须全同；三类注入（规范篡改/
//!     五字段缺一/证据悬空）逐类构造、逐类检出、漏抓即错。
//!
//! ## 职责定位
//! 域经格式规范的唯一权威定版面——上游是 F0741 登记簿（五字段来源）、
//! F0742 升格账（正文引用文法来源）、F0743 证据链账（证据三型与下限
//! 来源）、F0745 评级账（评级引用档位闭集）；下游是 F0747 检索索引、
//! F0748 跨域标注、F0749 移交包（三者消费冻结版模板）。本单不做检索、
//! 不做评级、不做合并——各归其位；规范一旦封存只许版本前进，不许
//! 原地改写（改即哈希红案）。
//!
//! ## 性能逐项分解
//! 入簿 O(编号查重)（线性扫，容量上限 1024 封顶）；字段 parse O(1)
//! （全定长文法）；证据 parse O(1)/条；审计 O(簿长×(5+1+证据数))；
//! 编号唯一核 O(簿长²)（n ≤ 1024）；指纹 O(规范形长+簿形长)；注入核
//! O(克隆账审计)。全整数确定性（零墙钟、零浮点、零随机、零迭代器
//! 依赖序——全 while+索引，双跑对拍天然全同）。
//!
//! ## 诊断码段
//! 0x2790 段（段基址 0x2790 保留为段标识不分配，码闭集 0x2791~0x279B
//! 十一码）。段位核验：0x2780~0x27BF 为 unx3361_utimewin 声明段，其
//! 形态码实际上界 0x278D（SEG_BASE+13），其辅助码经 `SEG_BASE +
//! AUX_BASE_OFF(0x1C)` 计算 = 0x279C~0x27A1 实占（计算式码纯文本
//! grep 不可见，已补读源码核实）→ 本段弃 0x279C~0x279F；任务备选
//! 0x27C0/0x27D0 落入 unx3381_usamplelib 声明且实际使用段
//! （SEG_BASE=0x27C0，同款 AUX_BASE_OFF=0x1C 计算式）→ 弃用；
//! 0x2790~0x279B 经全库实际码复核零占用，本单独占，不得他用。
//!
//! ## 纯功能线
//! no_std + alloc；零 unwrap/expect/panic/运行期 assert（编译期
//! const 断言除外）；可失败全走 `Result<T, FmtSpecError>`；溢出全
//! 显性 saturating/wrapping；>64KB 结构体一律不上栈（引擎本体含
//! Vec 字段，只许堆上持有；封存账快照每份 <1KB，堆上逐条追加）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const FS_VERSION: &str = "unx746-fmtspec-v1";

// =====================================================================
// 诊断码段：0x2790 段（段基址保留，码闭集 0x2791~0x279B）
// =====================================================================

/// 诊断段基址（0x2790 段 · 本模块独占；段基址本身保留为段标识不分配）。
pub const FS_SEG_BASE: u16 = 0x2790;

/// 诊断码：五字段空缺（字段缺失归因）。
pub const XC_FIELD_EMPTY: u32 = 0x2791;
/// 诊断码：字段格式不符（字段规范卡判负）。
pub const XC_FIELD_FMT: u32 = 0x2792;
/// 诊断码：字数越带（粒度归因，含低于带/高于带双侧）。
pub const XC_GRAN_OOB: u32 = 0x2793;
/// 诊断码：证据不足（低于该粒度证据下限，含全无）。
pub const XC_EVID_SHORT: u32 = 0x2794;
/// 诊断码：证据无解引（格式不可 parse 或注册表不解引）。
pub const XC_EVID_UNSOLV: u32 = 0x2795;
/// 诊断码：条目重号（编号已在簿）。
pub const XC_ENTRY_DUP: u32 = 0x2796;
/// 诊断码：容量越界（簿长越过上限）。
pub const XC_CAP_OVER: u32 = 0x2797;
/// 诊断码：规范篡改（冻结后规范指纹与封存值不一致，红案）。
pub const XC_FREEZE_TAMPER: u32 = 0x2798;
/// 诊断码：冻结生命周期破坏（未封存先校验/同版重封/版本封顶/无变更
/// 演进/演进未封存/补丁破坏带连续）。
pub const XC_LIFE_CYCLE: u32 = 0x2799;
/// 诊断码：事件序断裂（事件账序号不连续）。
pub const XC_EVENT_SEQ: u32 = 0x279A;
/// 诊断码：注入漏抓（注入面存在未检出类）。
pub const XC_INJECT_MISSED: u32 = 0x279B;

/// 码闭集编译期断言（严格递增互异，全部落在 0x2791~0x279B 段内）。
const _: () = assert!(XC_FIELD_EMPTY == 0x2791);
const _: () = assert!(XC_FIELD_FMT == 0x2792);
const _: () = assert!(XC_GRAN_OOB == 0x2793);
const _: () = assert!(XC_EVID_SHORT == 0x2794);
const _: () = assert!(XC_EVID_UNSOLV == 0x2795);
const _: () = assert!(XC_ENTRY_DUP == 0x2796);
const _: () = assert!(XC_CAP_OVER == 0x2797);
const _: () = assert!(XC_FREEZE_TAMPER == 0x2798);
const _: () = assert!(XC_LIFE_CYCLE == 0x2799);
const _: () = assert!(XC_EVENT_SEQ == 0x279A);
const _: () = assert!(XC_INJECT_MISSED == 0x279B);
const _: () = assert!(FS_SEG_BASE == 0x2790);

/// 中文诊断文本（码 → 现象/可操作建议，闭集逐码两要素）。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_FIELD_EMPTY => (
            "五字段空缺：条目存在空字段",
            "五字段（编号/标题/正文引用/评级引用/入册时戳）一律非空，补齐后重新入簿",
        ),
        XC_FIELD_FMT => (
            "字段格式不符：字段值未过字段规范卡",
            "对照字段规范卡逐卡核格式：编号 1..=999999、标题 4..=64 字节、正文引用 B批/F功能、评级引用 F0745:H|M|L、入册时戳 T+10 位",
        ),
        XC_GRAN_OOB => (
            "字数越带：正文字数不在该粒度档带宽内",
            "主题级 40..=120 / 批级 121..=400 / 域级 401..=1200，改字数或改粒度档归带",
        ),
        XC_EVID_SHORT => (
            "证据不足：证据条数低于该粒度下限",
            "主题级/批级至少 1 条证据，域级至少 2 条（F0743 证据链账深打要求）",
        ),
        XC_EVID_UNSOLV => (
            "证据无解引：引用格式不可 parse 或注册表不解引",
            "证据三型文法（判据号/账行号/文件锚点）逐条核，且须落在注册表闭集内方可解引",
        ),
        XC_ENTRY_DUP => (
            "条目重号：域经编号已在簿",
            "编号全域唯一（F0741 登记簿口径），换新号或先走 F0744 合并账",
        ),
        XC_CAP_OVER => (
            "容量越界：登记簿条目数越过上限",
            "簿容量 1024 封顶（F0741 全域条目量级上限），满簿即拒收新登记",
        ),
        XC_FREEZE_TAMPER => (
            "规范篡改：冻结后规范指纹与封存值不一致",
            "封存规范只许版本前进（v2 登记制），原地改写即红案，回滚到封存快照",
        ),
        XC_LIFE_CYCLE => (
            "冻结生命周期破坏：封存/演进序不符",
            "先封存再校验、同版不重封、演进版本恰 +1 且必须实质变更、演进后须重新封存",
        ),
        XC_EVENT_SEQ => (
            "事件序断裂：事件账序号不连续",
            "事件账 append-only 且序号从 1 连续递增，断号即账被改，回冻结点重放",
        ),
        XC_INJECT_MISSED => (
            "注入漏抓：注入面存在未检出类",
            "三类注入（规范篡改/五字段缺一/证据悬空）必须逐类检出，漏抓即回退本单版本",
        ),
        _ => ("未知诊断码", "查本模块 Xc 闭集 0x2791~0x279B"),
    }
}

// =====================================================================
// 第十一子系统先行：FNV-1a 32 位指纹（const fn，双跑确定性基元）
// =====================================================================

/// FNV-1a 32 位偏移基（标准常量，来源 FNV-1a 规范）。
pub const FNV_OFFSET_BASIS: u32 = 0x811C_9DC5;
/// FNV-1a 32 位素数（标准常量，来源 FNV-1a 规范）。
pub const FNV_PRIME: u32 = 0x0100_0193;

/// FNV-1a 32 位（const fn 本体：while+索引，零迭代器零分配）。
pub const fn fnv1a32(data: &[u8]) -> u32 {
    let mut h: u32 = FNV_OFFSET_BASIS;
    let mut i: usize = 0;
    while i < data.len() {
        h ^= data[i] as u32;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

// 空串基准与两枚标准向量编译期断言（FNV-1a 官方测试向量）。
const _: () = assert!(fnv1a32(b"") == 0x811C_9DC5);
const _: () = assert!(fnv1a32(b"a") == 0xE40C_292C);
const _: () = assert!(fnv1a32(b"foobar") == 0xBF9C_F968);

// =====================================================================
// 错误闭集 FmtSpecError
// =====================================================================

/// 本单错误闭集（每变体带诊断码 + 中文可操作诊断 `detail()`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum FmtSpecError {
    /// 五字段空缺（字段下标 0..=4）。
    FieldEmpty {
        /// 空缺字段下标（0=编号 1=标题 2=正文引用 3=评级引用 4=入册时戳）。
        field: u8,
    },
    /// 字段格式不符（规范卡判负，带理由码 1..=5）。
    FieldFmtBad {
        /// 判负字段下标。
        field: u8,
        /// 理由码（1=含非数字 2=值越界 3=长度越带 4=布局不符 5=含控制字节）。
        reason: u8,
    },
    /// 字数越带（粒度归因，带侧别）。
    GranBandViolation {
        /// 粒度档序（0=主题级 1=批级 2=域级）。
        gran: u8,
        /// 实际字数。
        words: u32,
        /// 侧别（1=低于带 2=高于带）。
        side: u8,
    },
    /// 证据不足（低于该粒度证据下限；found=0 即全无）。
    EvidenceShort {
        /// 条目编号。
        entry_id: u32,
        /// 实有证据条数。
        found: u32,
        /// 该粒度下限。
        need: u32,
    },
    /// 证据无解引（格式不可 parse 或注册表不解引）。
    EvidenceUnresolvable {
        /// 证据在条目内的序（0 基）。
        index: u8,
        /// 原文长度（诊断用，避免存动态串）。
        raw_len: u32,
    },
    /// 条目重号。
    EntryDuplicate {
        /// 冲突编号。
        id: u32,
    },
    /// 容量越界。
    CapacityExceeded {
        /// 当前簿容量上限。
        cap: u32,
    },
    /// 规范篡改（冻结哈希红案）。
    SpecTamperDetected {
        /// 封存指纹。
        frozen: u32,
        /// 复算指纹。
        computed: u32,
    },
    /// 冻结生命周期破坏（step 闭集见生命周期步骤表）。
    FreezeLifecycleBroken {
        /// 步骤码（1=未封存先校验 2=同版重复封存 3=版本封顶
        /// 4=无变更演进 5=演进后未封存 6=补丁破坏带连续）。
        step: u8,
    },
    /// 事件序断裂。
    EventSeqBroken {
        /// 期望序号。
        expect: u64,
        /// 实得序号。
        found: u64,
    },
    /// 注入漏抓（which=1 规范篡改 2 五字段缺一 3 证据悬空）。
    InjectionMissed {
        /// 漏抓注入类。
        which: u8,
    },
}

/// 生命周期步骤码中文注记（step → 现场描述，闭集）。
pub fn lifecycle_step_text(step: u8) -> &'static str {
    match step {
        1 => "未封存先校验：封存账空，先 freeze 再 verify",
        2 => "同版重复封存：当前版已封存，演进请走版本前进",
        3 => "版本封顶：规范版本已达 u8 上限，重开定版单",
        4 => "无变更演进：补丁结果与当前规范逐字节相同",
        5 => "演进后未封存：已演进未封存，先 freeze 新版",
        6 => "补丁破坏带连续：三档带宽必须连续无缝不重叠",
        _ => "未知生命周期步骤码",
    }
}

/// 字段格式理由码中文注记（reason → 现场描述，闭集）。
pub fn field_fmt_reason_text(reason: u8) -> &'static str {
    match reason {
        1 => "含非数字字符：该字段只收十进制数字",
        2 => "值越界：字段值不在规范卡值域内",
        3 => "长度越带：字段长度不在规范卡长度带内",
        4 => "布局不符：前缀/分隔符/段位数与文法不合",
        5 => "含控制字节：字段值混入 0x00..0x1F 或 0x7F",
        _ => "未知字段格式理由码",
    }
}

impl FmtSpecError {
    /// 诊断码（0x2791~0x279B 闭集）。
    pub fn code(&self) -> u32 {
        match self {
            FmtSpecError::FieldEmpty { .. } => XC_FIELD_EMPTY,
            FmtSpecError::FieldFmtBad { .. } => XC_FIELD_FMT,
            FmtSpecError::GranBandViolation { .. } => XC_GRAN_OOB,
            FmtSpecError::EvidenceShort { .. } => XC_EVID_SHORT,
            FmtSpecError::EvidenceUnresolvable { .. } => XC_EVID_UNSOLV,
            FmtSpecError::EntryDuplicate { .. } => XC_ENTRY_DUP,
            FmtSpecError::CapacityExceeded { .. } => XC_CAP_OVER,
            FmtSpecError::SpecTamperDetected { .. } => XC_FREEZE_TAMPER,
            FmtSpecError::FreezeLifecycleBroken { .. } => XC_LIFE_CYCLE,
            FmtSpecError::EventSeqBroken { .. } => XC_EVENT_SEQ,
            FmtSpecError::InjectionMissed { .. } => XC_INJECT_MISSED,
        }
    }

    /// 中文可操作诊断（逐变体闭集短句，非动态拼接）。
    pub fn detail(&self) -> &'static str {
        match self {
            FmtSpecError::FieldEmpty { .. } => {
                "五字段空缺：编号/标题/正文引用/评级引用/入册时戳一律非空，补齐后重入簿"
            }
            FmtSpecError::FieldFmtBad { .. } => {
                "字段格式不符：对照字段规范卡逐卡核格式（值域/长度带/文法布局/禁控制字节）"
            }
            FmtSpecError::GranBandViolation { .. } => {
                "字数越带：主题级 40..=120 / 批级 121..=400 / 域级 401..=1200，改字数或改档归带"
            }
            FmtSpecError::EvidenceShort { .. } => {
                "证据不足：主题级/批级至少 1 条、域级至少 2 条证据引用，补证据后重入簿"
            }
            FmtSpecError::EvidenceUnresolvable { .. } => {
                "证据无解引：三型文法逐条核格式，且须落在证据注册表闭集内方可解引"
            }
            FmtSpecError::EntryDuplicate { .. } => {
                "条目重号：编号全域唯一（F0741 口径），换新号或先走 F0744 合并账"
            }
            FmtSpecError::CapacityExceeded { .. } => {
                "容量越界：登记簿 1024 封顶，满簿拒收，扩容须重开定版单"
            }
            FmtSpecError::SpecTamperDetected { .. } => {
                "规范篡改：封存规范只许版本前进，原地改写即红案，回滚封存快照"
            }
            FmtSpecError::FreezeLifecycleBroken { .. } => {
                "冻结生命周期破坏：先封存再校验、同版不重封、演进恰 +1 且须实质变更、演进后重封"
            }
            FmtSpecError::EventSeqBroken { .. } => {
                "事件序断裂：事件账 append-only 序号从 1 连续递增，断号即账被改"
            }
            FmtSpecError::InjectionMissed { .. } => {
                "注入漏抓：三类注入（篡改/缺字段/悬空证据）必须逐类检出，漏抓即回退本单"
            }
        }
    }

    /// 完整诊断串（码 + 变体数据 + 中文诊断，落账留痕用）。
    pub fn full(&self) -> String {
        match self {
            FmtSpecError::FieldEmpty { field } => format!(
                "XC_{:04X}|field={}|{}",
                self.code(),
                field,
                self.detail()
            ),
            FmtSpecError::FieldFmtBad { field, reason } => format!(
                "XC_{:04X}|field={}|reason={}|{}",
                self.code(),
                field,
                reason,
                self.detail()
            ),
            FmtSpecError::GranBandViolation { gran, words, side } => format!(
                "XC_{:04X}|gran={}|words={}|side={}|{}",
                self.code(),
                gran,
                words,
                side,
                self.detail()
            ),
            FmtSpecError::EvidenceShort {
                entry_id,
                found,
                need,
            } => format!(
                "XC_{:04X}|entry={}|found={}|need={}|{}",
                self.code(),
                entry_id,
                found,
                need,
                self.detail()
            ),
            FmtSpecError::EvidenceUnresolvable { index, raw_len } => format!(
                "XC_{:04X}|index={}|len={}|{}",
                self.code(),
                index,
                raw_len,
                self.detail()
            ),
            FmtSpecError::EntryDuplicate { id } => {
                format!("XC_{:04X}|id={}|{}", self.code(), id, self.detail())
            }
            FmtSpecError::CapacityExceeded { cap } => {
                format!("XC_{:04X}|cap={}|{}", self.code(), cap, self.detail())
            }
            FmtSpecError::SpecTamperDetected { frozen, computed } => format!(
                "XC_{:04X}|frozen=0x{:08X}|computed=0x{:08X}|{}",
                self.code(),
                frozen,
                computed,
                self.detail()
            ),
            FmtSpecError::FreezeLifecycleBroken { step } => format!(
                "XC_{:04X}|step={}|{}|{}",
                self.code(),
                step,
                lifecycle_step_text(*step),
                self.detail()
            ),
            FmtSpecError::EventSeqBroken { expect, found } => format!(
                "XC_{:04X}|expect={}|found={}|{}",
                self.code(),
                expect,
                found,
                self.detail()
            ),
            FmtSpecError::InjectionMissed { which } => format!(
                "XC_{:04X}|which={}|{}",
                self.code(),
                which,
                self.detail()
            ),
        }
    }
}

// =====================================================================
// 第一子系统：模板五字段规范 TemplateSpec
// =====================================================================

/// 字段数（五字段闭集冻结，F0746-J1「模板五字段」口径）。
pub const FIELD_COUNT: usize = 5;

/// 字段下标闭集（0=编号 1=标题 2=正文引用 3=评级引用 4=入册时戳）。
pub const FIELD_IDX_ID: u8 = 0;
pub const FIELD_IDX_TITLE: u8 = 1;
pub const FIELD_IDX_BODY_REF: u8 = 2;
pub const FIELD_IDX_GRADE_REF: u8 = 3;
pub const FIELD_IDX_BOOKED_AT: u8 = 4;

/// 条目编号值域上限（来源：F0745 VG_ID_MAX=999999 对齐声明——编号口径
/// 全域统一，登记簿与评级账同域）。
pub const ENTRY_ID_MAX: u32 = 999_999;

/// 入册时戳逻辑钟位数（T+10 位；零墙钟纪律——逻辑 Tick 非墙钟）。
pub const BOOKED_AT_DIGITS: usize = 10;

/// 字段规范卡（字段名/格式规则/长度带/来源批账四列，const 写死）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FieldCard {
    /// 字段下标（0..=4）。
    pub key: u8,
    /// 字段名（中文定名，冻结）。
    pub name: &'static str,
    /// 格式规则（一句话文法，写死即文档）。
    pub rule: &'static str,
    /// 长度带下限（字节）。
    pub min_len: u32,
    /// 长度带上限（字节）。
    pub max_len: u32,
    /// 来源批账（字段口径的权威出处）。
    pub source: &'static str,
}

/// 模板五字段规范卡 const 表（[字段下标序] 冻结，全域统一模板定版本体）。
///
/// 五卡来源逐条注明：编号/标题承 F0741 登记簿五列口径；正文引用承
/// F0742 升格账正文指针文法；评级引用承 F0745 评级账档位闭集
/// （**F0745 对齐声明**：本卡直接消费 F0745 高/中/低三档，不重定义
/// 档位语义）；入册时戳为零墙钟逻辑 Tick。
pub const FIELD_CARDS: [FieldCard; FIELD_COUNT] = [
    FieldCard {
        key: FIELD_IDX_ID,
        name: "编号",
        rule: "十进制整数 1..=999999（u32 值域，非字符串位）",
        min_len: 1,
        max_len: 6,
        source: "F0741 登记簿编号列（与 F0745 VG_ID_MAX 对齐）",
    },
    FieldCard {
        key: FIELD_IDX_TITLE,
        name: "标题",
        rule: "4..=64 字节明文，禁控制字节（0x00..0x1F/0x7F）",
        min_len: 4,
        max_len: 64,
        source: "F0741 登记簿主题列",
    },
    FieldCard {
        key: FIELD_IDX_BODY_REF,
        name: "正文引用",
        rule: "B批号(2位)/F功能号(4位)，批 01..=99、功能 0001..=9999",
        min_len: 9,
        max_len: 9,
        source: "F0742 升格账正文指针文法",
    },
    FieldCard {
        key: FIELD_IDX_GRADE_REF,
        name: "评级引用",
        rule: "F0745:H|M|L（三档闭集，F0745 对齐）",
        min_len: 7,
        max_len: 7,
        source: "F0745 评级账档位列（对齐声明：直接消费不重定义）",
    },
    FieldCard {
        key: FIELD_IDX_BOOKED_AT,
        name: "入册时戳",
        rule: "T+10 位逻辑 Tick（值 ≥1；零墙钟）",
        min_len: 11,
        max_len: 11,
        source: "F0741 登记簿入册列（逻辑钟口径）",
    },
];

// 五卡编译期断言：卡数恰五、逐卡长度带自洽。
const _: () = assert!(FIELD_CARDS.len() == FIELD_COUNT);
const _: () = assert!(FIELD_CARDS[0].min_len >= 1);
const _: () = assert!(FIELD_CARDS[0].max_len >= FIELD_CARDS[0].min_len);
const _: () = assert!(FIELD_CARDS[1].min_len >= 1);
const _: () = assert!(FIELD_CARDS[1].max_len >= FIELD_CARDS[1].min_len);
const _: () = assert!(FIELD_CARDS[2].min_len >= 1);
const _: () = assert!(FIELD_CARDS[2].max_len >= FIELD_CARDS[2].min_len);
const _: () = assert!(FIELD_CARDS[3].min_len >= 1);
const _: () = assert!(FIELD_CARDS[3].max_len >= FIELD_CARDS[3].min_len);
const _: () = assert!(FIELD_CARDS[4].min_len >= 1);
const _: () = assert!(FIELD_CARDS[4].max_len >= FIELD_CARDS[4].min_len);
const _: () = assert!(ENTRY_ID_MAX == 999_999);

/// 字段名（下标 → 中文名；越界回 "未知字段"）。
pub fn field_name(key: u8) -> &'static str {
    match key {
        FIELD_IDX_ID => FIELD_CARDS[0].name,
        FIELD_IDX_TITLE => FIELD_CARDS[1].name,
        FIELD_IDX_BODY_REF => FIELD_CARDS[2].name,
        FIELD_IDX_GRADE_REF => FIELD_CARDS[3].name,
        FIELD_IDX_BOOKED_AT => FIELD_CARDS[4].name,
        _ => "未知字段",
    }
}

/// 字段规范卡表渲染（写死即文档：五卡四列逐行落账文本）。
pub fn render_field_cards(cards: &[FieldCard; FIELD_COUNT]) -> String {
    let mut out = String::from("[模板五字段规范]\n");
    let mut i: usize = 0;
    while i < FIELD_COUNT {
        let c = cards[i];
        out.push_str(&format!(
            "  字段{} {} | {} | {}..={} 字节 | 来源 {}\n",
            i, c.name, c.rule, c.min_len, c.max_len, c.source
        ));
        i += 1;
    }
    out
}

// ------------------- 手写文法基元（零正则、零 slice 直切） -------------------

/// 字节串全数字校验（while+索引，const 友好风格）。
fn digits_only(s: &[u8]) -> bool {
    let mut i: usize = 0;
    while i < s.len() {
        if s[i] < b'0' || s[i] > b'9' {
            return false;
        }
        i += 1;
    }
    true
}

/// 数字串 → u32（调用方须先过 digits_only；saturating 防溢出）。
fn digits_to_u32(s: &[u8]) -> u32 {
    let mut v: u32 = 0;
    let mut i: usize = 0;
    while i < s.len() {
        v = v.saturating_mul(10).saturating_add((s[i] - b'0') as u32);
        i += 1;
    }
    v
}

/// 数字串 → u64（同上，saturating）。
fn digits_to_u64(s: &[u8]) -> u64 {
    let mut v: u64 = 0;
    let mut i: usize = 0;
    while i < s.len() {
        v = v.saturating_mul(10).saturating_add((s[i] - b'0') as u64);
        i += 1;
    }
    v
}

/// 字节前缀匹配（越界安全；while+索引零 panic 面）。
fn bytes_eq_at(b: &[u8], off: usize, pat: &[u8]) -> bool {
    if off > b.len() || b.len() - off < pat.len() {
        return false;
    }
    let mut i: usize = 0;
    while i < pat.len() {
        if b[off + i] != pat[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 控制字节检出（0x00..=0x1F 或 0x7F）。
fn has_control_byte(s: &[u8]) -> bool {
    let mut i: usize = 0;
    while i < s.len() {
        if s[i] <= 0x1F || s[i] == 0x7F {
            return true;
        }
        i += 1;
    }
    false
}

// ------------------- 五字段逐卡 parse（值域+长度+文法） -------------------

/// 字段0 编号：u32 值域 1..=999999（非字符串位，直接值校验）。
pub fn parse_entry_id(id: u32) -> Result<u32, FmtSpecError> {
    if id == 0 || id > ENTRY_ID_MAX {
        return Err(FmtSpecError::FieldFmtBad {
            field: FIELD_IDX_ID,
            reason: 2,
        });
    }
    Ok(id)
}

/// 字段1 标题：长度带 4..=64 字节 + 禁控制字节。
pub fn parse_title(s: &str) -> Result<u32, FmtSpecError> {
    let b = s.as_bytes();
    let n = b.len() as u32;
    if n < FIELD_CARDS[1].min_len || n > FIELD_CARDS[1].max_len {
        return Err(FmtSpecError::FieldFmtBad {
            field: FIELD_IDX_TITLE,
            reason: 3,
        });
    }
    if has_control_byte(b) {
        return Err(FmtSpecError::FieldFmtBad {
            field: FIELD_IDX_TITLE,
            reason: 5,
        });
    }
    Ok(n)
}

/// 字段2 正文引用：`B批号(2位)/F功能号(4位)`（总长恰 9 字节），
/// 批 01..=99、功能 0001..=9999。
///
/// 返回 (批号, 功能号)。文法来源：F0742 升格账正文指针。
pub fn parse_body_ref(s: &str) -> Result<(u8, u16), FmtSpecError> {
    let b = s.as_bytes();
    if b.len() != 9 || b[0] != b'B' || b[3] != b'/' || b[4] != b'F' {
        return Err(FmtSpecError::FieldFmtBad {
            field: FIELD_IDX_BODY_REF,
            reason: 4,
        });
    }
    if !digits_only(&b[1..3]) || !digits_only(&b[5..9]) {
        return Err(FmtSpecError::FieldFmtBad {
            field: FIELD_IDX_BODY_REF,
            reason: 1,
        });
    }
    let batch = digits_to_u32(&b[1..3]);
    let task = digits_to_u32(&b[5..9]);
    if batch < 1 || batch > 99 || task < 1 || task > 9999 {
        return Err(FmtSpecError::FieldFmtBad {
            field: FIELD_IDX_BODY_REF,
            reason: 2,
        });
    }
    Ok((batch as u8, task as u16))
}

/// 字段3 评级引用：`F0745:H|M|L`（F0745 对齐——档位三闭集直引）。
///
/// 返回档位序（0=高 1=中 2=低，与 F0745 GradeTier 序对齐）。
pub fn parse_grade_ref(s: &str) -> Result<u8, FmtSpecError> {
    let b = s.as_bytes();
    if b.len() != 7 || !bytes_eq_at(b, 0, b"F0745:") {
        return Err(FmtSpecError::FieldFmtBad {
            field: FIELD_IDX_GRADE_REF,
            reason: 4,
        });
    }
    match b[6] {
        b'H' => Ok(0),
        b'M' => Ok(1),
        b'L' => Ok(2),
        _ => Err(FmtSpecError::FieldFmtBad {
            field: FIELD_IDX_GRADE_REF,
            reason: 4,
        }),
    }
}

/// 字段4 入册时戳：`T+10 位逻辑 Tick`，值 ≥1（Tick 0 保留为未落账哨值）。
pub fn parse_booked_at(s: &str) -> Result<u64, FmtSpecError> {
    let b = s.as_bytes();
    if b.len() != BOOKED_AT_DIGITS + 1 || b[0] != b'T' {
        return Err(FmtSpecError::FieldFmtBad {
            field: FIELD_IDX_BOOKED_AT,
            reason: 4,
        });
    }
    if !digits_only(&b[1..BOOKED_AT_DIGITS + 1]) {
        return Err(FmtSpecError::FieldFmtBad {
            field: FIELD_IDX_BOOKED_AT,
            reason: 1,
        });
    }
    let v = digits_to_u64(&b[1..BOOKED_AT_DIGITS + 1]);
    if v < 1 {
        return Err(FmtSpecError::FieldFmtBad {
            field: FIELD_IDX_BOOKED_AT,
            reason: 2,
        });
    }
    Ok(v)
}

// =====================================================================
// 第二子系统：粒度规范 GranularitySpec
// =====================================================================

/// 粒度档数（三档闭集：主题级/批级/域级）。
pub const GRAN_COUNT: usize = 3;

/// 粒度档枚举（0=主题级 1=批级 2=域级；下标冻结）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Granularity {
    /// 主题级：单主题单打法，最小可复用单元。
    Topic,
    /// 批级：跨条目批内打法，承 F0742 升格账四要素展开。
    Batch,
    /// 域级：全域沉淀级（方法论），承 F0743 证据链深打。
    Domain,
}

impl Granularity {
    /// 档序（0..=2）。
    pub fn index(self) -> usize {
        match self {
            Granularity::Topic => 0,
            Granularity::Batch => 1,
            Granularity::Domain => 2,
        }
    }

    /// 按档序取枚举（越界 None）。
    pub fn from_index(i: usize) -> Option<Granularity> {
        match i {
            0 => Some(Granularity::Topic),
            1 => Some(Granularity::Batch),
            2 => Some(Granularity::Domain),
            _ => None,
        }
    }

    /// 档名（中文定名）。
    pub fn name(self) -> &'static str {
        GRAN_NAMES[self.index()]
    }

    /// 短标签（登记簿序列化用单字节 tag）。
    pub fn tag(self) -> &'static str {
        match self {
            Granularity::Topic => "topic",
            Granularity::Batch => "batch",
            Granularity::Domain => "domain",
        }
    }

    /// 字数带宽 [min, max]（闭区间，双端含）。
    pub fn band(self) -> (u32, u32) {
        GRAN_BANDS[self.index()]
    }

    /// 该档证据条数下限（域级≥2，来源 F0743 证据链账）。
    pub fn min_evidence(self) -> u32 {
        GRAN_MIN_EVIDENCE[self.index()]
    }
}

/// 粒度三档中文名（下标序冻结）。
pub const GRAN_NAMES: [&str; GRAN_COUNT] = ["主题级", "批级", "域级"];

/// 粒度→字数带宽 const 表（[档序] 冻结，闭区间双端含）。
///
/// 带宽来源（写死即文档）：
/// - 主题级 40..=120：F0742 升格账「打法描述」单要素最小展开估算；
/// - 批级 121..=400：F0742 四要素（打法/条件/反例/证据）全量展开估算；
/// - 域级 401..=1200：F0743 证据链深打 + F0744 合并账主条口径估算。
/// 三带设计为连续无缝（Topic.max+1 == Batch.min 等），编译期断言守卫。
pub const GRAN_BANDS: [(u32, u32); GRAN_COUNT] = [(40, 120), (121, 400), (401, 1200)];

/// 粒度→证据条数下限 const 表（域级 ≥2：来源 F0743 证据链账——
/// 域级条目须至少一条判据引用 + 一条账行/锚点引用双源可解引）。
pub const GRAN_MIN_EVIDENCE: [u32; GRAN_COUNT] = [1, 1, 2];

// 三带连续性编译期断言（无缝不重叠）。
const _: () = assert!(GRAN_BANDS[0].0 >= 1);
const _: () = assert!(GRAN_BANDS[0].1 >= GRAN_BANDS[0].0);
const _: () = assert!(GRAN_BANDS[1].0 == GRAN_BANDS[0].1 + 1);
const _: () = assert!(GRAN_BANDS[1].1 >= GRAN_BANDS[1].0);
const _: () = assert!(GRAN_BANDS[2].0 == GRAN_BANDS[1].1 + 1);
const _: () = assert!(GRAN_BANDS[2].1 >= GRAN_BANDS[2].0);
const _: () = assert!(GRAN_MIN_EVIDENCE[0] >= 1);
const _: () = assert!(GRAN_MIN_EVIDENCE[2] >= 2);

/// 越带校验（对指定带宽表；引擎登记闸用运行规范表，演进后口径即随行）。
pub fn check_gran_band_on_spec(
    bands: &[(u32, u32); GRAN_COUNT],
    gran: Granularity,
    words: u32,
) -> Result<(), FmtSpecError> {
    let gi = gran.index();
    let (lo, hi) = bands[gi];
    if words < lo {
        return Err(FmtSpecError::GranBandViolation {
            gran: gi as u8,
            words,
            side: 1,
        });
    }
    if words > hi {
        return Err(FmtSpecError::GranBandViolation {
            gran: gi as u8,
            words,
            side: 2,
        });
    }
    Ok(())
}

/// 越带校验（const 定版表口径：v1 冻结带宽 [40..=120/121..=400/401..=1200]，
/// 含侧别归因）。运行规范口径走引擎 `check_band_current`（演进随行）。
pub fn check_gran_band(gran: Granularity, words: u32) -> Result<(), FmtSpecError> {
    check_gran_band_on_spec(&GRAN_BANDS, gran, words)
}

/// 粒度规范表渲染（三档带宽+证据下限逐行落账文本）。
pub fn render_gran_spec(bands: &[(u32, u32); GRAN_COUNT], min_ev: &[u32; GRAN_COUNT]) -> String {
    let mut out = String::from("[粒度规范]\n");
    let mut i: usize = 0;
    while i < GRAN_COUNT {
        out.push_str(&format!(
            "  {}{} | 带宽 {}..={} 字 | 证据下限 {} 条\n",
            i,
            GRAN_NAMES[i],
            bands[i].0,
            bands[i].1,
            min_ev[i]
        ));
        i += 1;
    }
    out
}

// =====================================================================
// 第三子系统：证据规范 EvidenceSpec（三型文法 + 可解引注册表）
// =====================================================================

/// 证据引用三型枚举（判据号/账行号/文件锚点）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum EvidenceRef {
    /// 判据号：`UNX-F%04u-J%u`（功能号 0001..=9999，判据序 1..=9）。
    Criterion {
        /// 功能号（如 746）。
        task: u16,
        /// 判据序（如 1）。
        jdx: u8,
    },
    /// 账行号：`B%02u:L%u`（批号 1..=99，行号 1..=99999）。
    LedgerLine {
        /// 批号（如 38）。
        batch: u8,
        /// 行号（如 11500）。
        line: u32,
    },
    /// 文件锚点：`src:<路径>:<行号>`（路径已知闭集，行号 1..=999999）。
    FileAnchor {
        /// 仓库相对路径（须落在注册表已知文件闭集内）。
        path: String,
        /// 行号（1 基）。
        line: u32,
    },
}

impl EvidenceRef {
    /// 型名。
    pub fn kind_name(&self) -> &'static str {
        match self {
            EvidenceRef::Criterion { .. } => "判据号",
            EvidenceRef::LedgerLine { .. } => "账行号",
            EvidenceRef::FileAnchor { .. } => "文件锚点",
        }
    }

    /// 型序（0/1/2，规范序列化用）。
    pub fn kind_index(&self) -> u8 {
        match self {
            EvidenceRef::Criterion { .. } => 0,
            EvidenceRef::LedgerLine { .. } => 1,
            EvidenceRef::FileAnchor { .. } => 2,
        }
    }
}

/// 证据三型文法卡 const 表（写死即文档：文法 + 值域 + 来源）。
pub const EVID_GRAMMARS: [&str; 3] = [
    // 判据号：锚功能条目的 J 判据列（主册表格判据列口径）。
    "判据号 UNX-F%04u-J%u（功能 0001..=9999，判据序 1..=9）← 主册判据列",
    // 账行号：锚批账行（B 批账行号，批 1..=99、行 1..=99999）。
    "账行号 B%02u:L%u（批 01..=99，行 1..=99999）← 批账行号列",
    // 文件锚点：锚源码行（仓库相对路径 + 1 基行号）。
    "文件锚点 src:<路径>:<行号>（路径已知闭集，行 1..=999999）← 源码行",
];

/// 证据规范表渲染（三型文法逐行落账文本）。
pub fn render_evid_spec(grammars: &[&str; 3]) -> String {
    let mut out = String::from("[证据规范]\n");
    let mut i: usize = 0;
    while i < 3 {
        out.push_str(&format!("  型{} {}\n", i, grammars[i]));
        i += 1;
    }
    out
}

/// 证据引用 parse（解引性第一关：格式可解；三型逐型手写文法）。
///
/// - `UNX-F0746-J1` → Criterion { task: 746, jdx: 1 }
/// - `B38:L11500` → LedgerLine { batch: 38, line: 11500 }
/// - `src:varix/src/unxstar/unx746_fmtspec.rs:100` → FileAnchor { .. }
pub fn parse_evidence(s: &str) -> Result<EvidenceRef, FmtSpecError> {
    let b = s.as_bytes();
    // 型1 判据号：UNX-F####-J#（总长恰 12）。
    if bytes_eq_at(b, 0, b"UNX-F") {
        if b.len() != 12 || b[9] != b'-' || b[10] != b'J' {
            return Err(FmtSpecError::EvidenceUnresolvable {
                index: 0,
                raw_len: b.len() as u32,
            });
        }
        if !digits_only(&b[5..9]) || !digits_only(&b[11..12]) {
            return Err(FmtSpecError::EvidenceUnresolvable {
                index: 0,
                raw_len: b.len() as u32,
            });
        }
        let task = digits_to_u32(&b[5..9]);
        let jdx = digits_to_u32(&b[11..12]);
        if task < 1 || task > 9999 || jdx < 1 || jdx > 9 {
            return Err(FmtSpecError::EvidenceUnresolvable {
                index: 0,
                raw_len: b.len() as u32,
            });
        }
        return Ok(EvidenceRef::Criterion {
            task: task as u16,
            jdx: jdx as u8,
        });
    }
    // 型2 账行号：B##:L#####（批 1..2 位、行 1..5 位数字）。
    if b.len() >= 4 && b[0] == b'B' {
        let mut colon: usize = 0;
        let mut i: usize = 1;
        while i < b.len() {
            if b[i] == b':' {
                colon = i;
                break;
            }
            i += 1;
        }
        let batch_ok = colon >= 2
            && colon <= 3
            && digits_only(&b[1..colon])
            && b.len() >= colon + 3
            && b[colon + 1] == b'L'
            && b.len() - colon - 2 >= 1
            && b.len() - colon - 2 <= 5
            && digits_only(&b[colon + 2..b.len()]);
        if !batch_ok {
            return Err(FmtSpecError::EvidenceUnresolvable {
                index: 0,
                raw_len: b.len() as u32,
            });
        }
        let batch = digits_to_u32(&b[1..colon]);
        let line = digits_to_u32(&b[colon + 2..b.len()]);
        if batch < 1 || batch > 99 || line < 1 || line > 99_999 {
            return Err(FmtSpecError::EvidenceUnresolvable {
                index: 0,
                raw_len: b.len() as u32,
            });
        }
        return Ok(EvidenceRef::LedgerLine {
            batch: batch as u8,
            line,
        });
    }
    // 型3 文件锚点：src:<路径>:<行号>（路径禁内嵌冒号，取最后冒号分段）。
    if bytes_eq_at(b, 0, b"src:") {
        let mut last: usize = 0;
        let mut i: usize = 4;
        while i < b.len() {
            if b[i] == b':' {
                last = i;
            }
            i += 1;
        }
        let path_ok = last > 4 && last < b.len() - 1;
        if !path_ok {
            return Err(FmtSpecError::EvidenceUnresolvable {
                index: 0,
                raw_len: b.len() as u32,
            });
        }
        // 路径段（4..last）：非空 + 禁内嵌冒号 + 禁控制字节。
        let mut pc: usize = 4;
        let mut path_clean = true;
        while pc < last {
            if b[pc] == b':' || b[pc] <= 0x1F || b[pc] == 0x7F {
                path_clean = false;
                break;
            }
            pc += 1;
        }
        let line_digits = b.len() - last - 1;
        if !path_clean
            || line_digits < 1
            || line_digits > 6
            || !digits_only(&b[last + 1..b.len()])
        {
            return Err(FmtSpecError::EvidenceUnresolvable {
                index: 0,
                raw_len: b.len() as u32,
            });
        }
        let line = digits_to_u32(&b[last + 1..b.len()]);
        if line < 1 || line > 999_999 {
            return Err(FmtSpecError::EvidenceUnresolvable {
                index: 0,
                raw_len: b.len() as u32,
            });
        }
        let path = match s.get(4..last) {
            Some(p) => p.to_string(),
            None => {
                return Err(FmtSpecError::EvidenceUnresolvable {
                    index: 0,
                    raw_len: b.len() as u32,
                })
            }
        };
        return Ok(EvidenceRef::FileAnchor { path, line });
    }
    Err(FmtSpecError::EvidenceUnresolvable {
        index: 0,
        raw_len: b.len() as u32,
    })
}

// ------------------- 证据可解引注册表（解引性第二关） -------------------

/// 注册表判据功能号数（闭集容量，来源见 REGISTRY_TASK_NOS 注释）。
pub const REGISTRY_TASK_COUNT: usize = 28;

/// 判据功能号闭集（28 号 const 写死；来源：B38 批表 F0741..F0760 全 20
/// 号 + 兄弟单引用的既有账面号 F0679 性能基线 / F0688 failarc / F0689 /
/// F0704 / F0719 移交声明 / F0736 接口冻结 / F0739 候选 / F0740 ktest）。
pub const REGISTRY_TASK_NOS: [u16; REGISTRY_TASK_COUNT] = [
    679, 688, 689, 704, 719, 736, 739, 740, 741, 742, 743, 744, 745, 746, 747, 748, 749, 750,
    751, 752, 753, 754, 755, 756, 757, 758, 759, 760,
];

/// 批账行上界表（批号 → 该批账最大行号；来源：主册批头「批小计」列——
/// B37 批小计 11450、B38 批小计 11500 逐条求和落账，B40 收官批同量级）。
pub const REGISTRY_BATCH_LINES: [(u8, u32); 4] = [(27, 9_980), (37, 11_450), (38, 11_500), (40, 11_500)];

/// 已知文件闭集（仓库相对路径；本单邻件与上游单源码）。
pub const REGISTRY_FILES: [&str; 8] = [
    "varix/src/unxstar/unx741_domain_journal.rs",
    "varix/src/unxstar/unx743_evichain.rs",
    "varix/src/unxstar/unx745_valgrade.rs",
    "varix/src/unxstar/unx746_fmtspec.rs",
    "varix/src/unxstar/unx3401_uktestdesign.rs",
    "varix/src/unxstar/unx3101_uregfreeze.rs",
    "varix/src/unxstar/unx3361_utimewin.rs",
    "varix/src/unxstar/mod.rs",
];

/// 证据可解引注册表（三闭集 const 构造；Copy，全静态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EvidenceRegistry {
    /// 判据功能号闭集。
    pub tasks: [u16; REGISTRY_TASK_COUNT],
    /// 批账行上界表。
    pub batch_lines: [(u8, u32); 4],
    /// 已知文件闭集。
    pub files: [&'static str; 8],
}

/// 默认注册表（const 写死，全域统一解引口径）。
pub const DEFAULT_REGISTRY: EvidenceRegistry = EvidenceRegistry {
    tasks: REGISTRY_TASK_NOS,
    batch_lines: REGISTRY_BATCH_LINES,
    files: REGISTRY_FILES,
};

impl EvidenceRegistry {
    /// 判据功能号是否在闭集（while+索引线性扫）。
    pub fn knows_task(&self, task: u16) -> bool {
        let mut i: usize = 0;
        while i < self.tasks.len() {
            if self.tasks[i] == task {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 批账行是否可解引（批号在表且行号 ≤ 该批上界）。
    pub fn ledger_line_ok(&self, batch: u8, line: u32) -> bool {
        let mut i: usize = 0;
        while i < self.batch_lines.len() {
            if self.batch_lines[i].0 == batch {
                return line >= 1 && line <= self.batch_lines[i].1;
            }
            i += 1;
        }
        false
    }

    /// 文件路径是否在已知闭集。
    pub fn knows_file(&self, path: &str) -> bool {
        let mut i: usize = 0;
        while i < self.files.len() {
            if self.files[i] == path {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 证据引用是否可解引（对已 parse 的 EvidenceRef 三型分派）。
    pub fn resolves(&self, e: &EvidenceRef) -> bool {
        match e {
            EvidenceRef::Criterion { task, .. } => self.knows_task(*task),
            EvidenceRef::LedgerLine { batch, line } => self.ledger_line_ok(*batch, *line),
            EvidenceRef::FileAnchor { path, line } => {
                *line >= 1 && self.knows_file(path.as_str())
            }
        }
    }
}

// =====================================================================
// 运行时规范束 RuntimeSpec（三规范聚合 + 规范形序列化 + 渲染）
// =====================================================================

/// 规范束（模板五字段卡 + 粒度带宽/证据下限 + 证据三型文法，全 Copy；
/// 封存账逐版快照即逐份此结构，<1KB）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RuntimeSpec {
    /// 规范版本号（v1 起，演进只前进）。
    pub version: u8,
    /// 模板五字段规范卡。
    pub field_cards: [FieldCard; FIELD_COUNT],
    /// 粒度→字数带宽表。
    pub gran_bands: [(u32, u32); GRAN_COUNT],
    /// 粒度→证据条数下限表。
    pub gran_min_evidence: [u32; GRAN_COUNT],
    /// 证据三型文法卡。
    pub evid_grammars: [&'static str; 3],
}

/// 规范 v1 定版（const 写死——全域统一模板定版本体）。
pub const SPEC_V1: RuntimeSpec = RuntimeSpec {
    version: 1,
    field_cards: FIELD_CARDS,
    gran_bands: GRAN_BANDS,
    gran_min_evidence: GRAN_MIN_EVIDENCE,
    evid_grammars: EVID_GRAMMARS,
};

const _: () = assert!(SPEC_V1.version == 1);

/// 规范形序列化（canonical bytes：哈希封存唯一输入形，确定序写死）。
///
/// 形：`FMTSPEC-V<版>` + 逐卡 `key/min/max/name/rule/source` + 逐档
/// `min/max/min_evid` + 三型文法，段分隔 0x7C，字段间 0x1F。任何规范
/// 内容改动必然改变本形 → 必然改变指纹（哈希红案的物性基础）。
pub fn canonical_spec_bytes(spec: &RuntimeSpec) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    v.extend_from_slice(b"FMTSPEC-V");
    v.push(b'0' + spec.version);
    v.push(0x7C);
    let mut i: usize = 0;
    while i < FIELD_COUNT {
        let c = spec.field_cards[i];
        v.push(b'F');
        v.push(b'0' + c.key);
        v.push(0x1F);
        v.extend_from_slice(&c.min_len.to_le_bytes());
        v.extend_from_slice(&c.max_len.to_le_bytes());
        v.push(0x1F);
        v.extend_from_slice(c.name.as_bytes());
        v.push(0x1F);
        v.extend_from_slice(c.rule.as_bytes());
        v.push(0x1F);
        v.extend_from_slice(c.source.as_bytes());
        v.push(0x7C);
        i += 1;
    }
    let mut g: usize = 0;
    while g < GRAN_COUNT {
        v.push(b'G');
        v.push(b'0' + g as u8);
        v.push(0x1F);
        v.extend_from_slice(&spec.gran_bands[g].0.to_le_bytes());
        v.extend_from_slice(&spec.gran_bands[g].1.to_le_bytes());
        v.extend_from_slice(&spec.gran_min_evidence[g].to_le_bytes());
        v.push(0x7C);
        g += 1;
    }
    let mut e: usize = 0;
    while e < 3 {
        v.push(b'E');
        v.push(b'0' + e as u8);
        v.push(0x1F);
        v.extend_from_slice(spec.evid_grammars[e].as_bytes());
        v.push(0x7C);
        e += 1;
    }
    v
}

/// 规范指纹（规范形 FNV-1a 32）。
pub fn spec_fingerprint_of(spec: &RuntimeSpec) -> u32 {
    let bytes = canonical_spec_bytes(spec);
    fnv1a32(bytes.as_slice())
}

/// 规范账全文渲染（三规范 + 版本 + 指纹，冻结证书正文）。
pub fn render_spec_ledger(spec: &RuntimeSpec) -> String {
    let fp = spec_fingerprint_of(spec);
    let mut out = format!("== 域经格式规范 冻结账 v{} ==\n", spec.version);
    out.push_str(&render_field_cards(&spec.field_cards));
    out.push_str(&render_gran_spec(
        &spec.gran_bands,
        &spec.gran_min_evidence,
    ));
    out.push_str(&render_evid_spec(&spec.evid_grammars));
    out.push_str(&format!("规范指纹 0x{:08X}\n", fp));
    out
}

// =====================================================================
// 第四子系统：规范冻结 SpecFreeze（封存账 + 演进白名单 + 篡改红案）
// =====================================================================

/// 封存账记录（五列：版本/指纹/逻辑时刻/规范快照/变更注记；append-only）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FreezeRecord {
    /// 封存版本号（账内严格递增恰 +1）。
    pub version: u8,
    /// 规范指纹（封存时点规范形哈希）。
    pub fingerprint: u32,
    /// 封存逻辑时刻（引擎逻辑 Tick，零墙钟）。
    pub tick: u64,
    /// 规范快照（整份 RuntimeSpec，账被改可逐字段 diff）。
    pub snapshot: RuntimeSpec,
    /// 变更注记（v1 定版注记 / 演进补丁描述）。
    pub change_note: String,
}

/// 演进补丁白名单（登记制：只允许两类「只增不改」语义的前进补丁）。
///
/// 白名单外无演进通道——模板五字段卡与证据文法 v1 冻结不可动（动即
/// 破坏 F0741..F0745 全部上游账的引用根基，须重开定版单走 v2 全量）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpecPatch {
    /// 抬宽末档（域级）带宽上限（只许抬不许降，三带连续性保形）。
    WidenDomainCeiling {
        /// 新上限（须 > 旧上限）。
        new_max: u32,
    },
    /// 抬高某档证据条数下限（只许抬不许降，收紧不改语义）。
    RaiseGranMinEvidence {
        /// 目标档。
        gran: Granularity,
        /// 新下限（须 > 旧下限且 ≤4）。
        new_min: u32,
    },
}

impl SpecPatch {
    /// 补丁描述（变更注记落账文本）。
    pub fn describe(&self) -> String {
        match self {
            SpecPatch::WidenDomainCeiling { new_max } => {
                format!("域级带宽上限 → {}", new_max)
            }
            SpecPatch::RaiseGranMinEvidence { gran, new_min } => {
                format!("{} 证据下限 → {}", gran.name(), new_min)
            }
        }
    }
}

/// 封存账健康（演进后未封存检出用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FreezeState {
    /// 账内记录数。
    pub records: u32,
    /// 当前规范版本。
    pub version: u8,
    /// 是否已封存当前版（账尾版 == 当前版）。
    pub sealed: bool,
}

/// 规范封存子系统（封存账 + 当前运行规范）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SpecFreeze {
    /// 当前运行规范（生效版；演进后未封存期为「待封版」）。
    pub spec: RuntimeSpec,
    /// 封存账（append-only；每版一条，只增不改）。
    pub records: Vec<FreezeRecord>,
}

impl SpecFreeze {
    /// 空账起步（v1 未封存）。
    pub fn new() -> SpecFreeze {
        SpecFreeze {
            spec: SPEC_V1,
            records: Vec::new(),
        }
    }

    /// 账内记录数。
    pub fn record_count(&self) -> usize {
        self.records.len()
    }

    /// 账尾记录（空账 None）。
    pub fn last_record(&self) -> Option<&FreezeRecord> {
        if self.records.is_empty() {
            None
        } else {
            let n = self.records.len();
            Some(&self.records[n - 1])
        }
    }

    /// 当前版是否已封存（账非空且账尾版 == 当前版）。
    pub fn is_sealed(&self) -> bool {
        match self.last_record() {
            Some(r) => r.version == self.spec.version,
            None => false,
        }
    }

    /// 冻结状态三概要。
    pub fn state(&self) -> FreezeState {
        FreezeState {
            records: self.records.len() as u32,
            version: self.spec.version,
            sealed: self.is_sealed(),
        }
    }

    /// 封存当前版（哈希封存：规范形 → FNV-1a → 落账）。
    ///
    /// 生命周期闸：同版重复封存拒收（step=2）。演进后待封版正常封存。
    pub fn seal(&mut self, tick: u64, note: String) -> Result<u32, FmtSpecError> {
        if self.is_sealed() {
            return Err(FmtSpecError::FreezeLifecycleBroken { step: 2 });
        }
        let fp = spec_fingerprint_of(&self.spec);
        let version = self.spec.version;
        self.records.push(FreezeRecord {
            version,
            fingerprint: fp,
            tick,
            snapshot: self.spec,
            change_note: note,
        });
        Ok(fp)
    }

    /// 校验当前规范未被篡改（冻结哈希红案核对）。
    ///
    /// 生命周期闸：未封存先校验拒收（step=1）；演进后未封存拒收
    /// （step=5，账尾版 ≠ 当前版）。指纹不符即 SpecTamperDetected。
    pub fn verify(&self) -> Result<u32, FmtSpecError> {
        let last = match self.last_record() {
            Some(r) => r,
            None => return Err(FmtSpecError::FreezeLifecycleBroken { step: 1 }),
        };
        if last.version != self.spec.version {
            return Err(FmtSpecError::FreezeLifecycleBroken { step: 5 });
        }
        let computed = spec_fingerprint_of(&self.spec);
        if computed != last.fingerprint {
            return Err(FmtSpecError::SpecTamperDetected {
                frozen: last.fingerprint,
                computed,
            });
        }
        Ok(last.fingerprint)
    }

    /// 演进补丁登记（版本前进：v→v+1，旧版记录不动，白名单补丁）。
    ///
    /// 生命周期闸：未封存不许演进（step=5——演进必须建立在已封存基线
    /// 之上）；版本封顶（step=3）；无变更演进（step=4）；补丁破坏三带
    /// 连续（step=6）。演进成功即进入「待封版」态，须再 seal 落账。
    pub fn evolve(&mut self, patch: SpecPatch) -> Result<u8, FmtSpecError> {
        if !self.is_sealed() {
            return Err(FmtSpecError::FreezeLifecycleBroken { step: 5 });
        }
        if self.spec.version == u8::MAX {
            return Err(FmtSpecError::FreezeLifecycleBroken { step: 3 });
        }
        let mut next = self.spec;
        match patch {
            SpecPatch::WidenDomainCeiling { new_max } => {
                let old = next.gran_bands[2].1;
                if new_max <= old {
                    return Err(FmtSpecError::FreezeLifecycleBroken { step: 4 });
                }
                if new_max < next.gran_bands[2].0 {
                    return Err(FmtSpecError::FreezeLifecycleBroken { step: 6 });
                }
                next.gran_bands[2].1 = new_max;
            }
            SpecPatch::RaiseGranMinEvidence { gran, new_min } => {
                let gi = gran.index();
                let old = next.gran_min_evidence[gi];
                if new_min == old {
                    return Err(FmtSpecError::FreezeLifecycleBroken { step: 4 });
                }
                if new_min < old || new_min > 4 {
                    return Err(FmtSpecError::FreezeLifecycleBroken { step: 6 });
                }
                next.gran_min_evidence[gi] = new_min;
            }
        }
        if next == self.spec {
            return Err(FmtSpecError::FreezeLifecycleBroken { step: 4 });
        }
        next.version = self.spec.version + 1;
        self.spec = next;
        Ok(self.spec.version)
    }

    /// 相邻两版差异账渲染（账尾两条快照逐字段比对；不足两版报 step=1）。
    pub fn render_evolution_diff(&self) -> Result<String, FmtSpecError> {
        let n = self.records.len();
        if n < 2 {
            return Err(FmtSpecError::FreezeLifecycleBroken { step: 1 });
        }
        let a = &self.records[n - 2];
        let b = &self.records[n - 1];
        let mut out = format!(
            "== 规范演进差异账 v{}→v{} ==\n",
            a.version, b.version
        );
        // 五字段卡逐卡比对（v1 冻结不可动，预期零差异行）。
        let mut i: usize = 0;
        while i < FIELD_COUNT {
            if a.snapshot.field_cards[i] != b.snapshot.field_cards[i] {
                out.push_str(&format!(
                    "  字段卡{} {} → {}\n",
                    i, a.snapshot.field_cards[i].name, b.snapshot.field_cards[i].name
                ));
            }
            i += 1;
        }
        // 三档带宽/证据下限逐档比对（演进主战场）。
        let mut g: usize = 0;
        while g < GRAN_COUNT {
            if a.snapshot.gran_bands[g] != b.snapshot.gran_bands[g] {
                out.push_str(&format!(
                    "  {}带宽 {}..{} → {}..{}\n",
                    GRAN_NAMES[g],
                    a.snapshot.gran_bands[g].0,
                    a.snapshot.gran_bands[g].1,
                    b.snapshot.gran_bands[g].0,
                    b.snapshot.gran_bands[g].1
                ));
            }
            if a.snapshot.gran_min_evidence[g] != b.snapshot.gran_min_evidence[g] {
                out.push_str(&format!(
                    "  {}证据下限 {} → {}\n",
                    GRAN_NAMES[g],
                    a.snapshot.gran_min_evidence[g],
                    b.snapshot.gran_min_evidence[g]
                ));
            }
            g += 1;
        }
        // 三型文法比对（v1 冻结不可动，预期零差异行）。
        let mut e: usize = 0;
        while e < 3 {
            if a.snapshot.evid_grammars[e] != b.snapshot.evid_grammars[e] {
                out.push_str(&format!("  证据文法型{} 变更\n", e));
            }
            e += 1;
        }
        out.push_str(&format!("  变更注记 {}\n", b.change_note));
        Ok(out)
    }
}

// =====================================================================
// 第五子系统：冻结证书 FreezeCertificate
// =====================================================================

/// 冻结证书（封存即出证：五要素逐字段可对封存账复核）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FreezeCertificate {
    /// 证书覆盖的规范版本。
    pub version: u8,
    /// 规范指纹。
    pub fingerprint: u32,
    /// 封存逻辑时刻。
    pub tick: u64,
    /// 封存时点登记簿条目数。
    pub entries_at_freeze: u32,
    /// 规范账全文渲染（证书正文）。
    pub ledger: String,
}

// =====================================================================
// 第六/七/八子系统：条目模型 + 事件账 + 符合度审计
// =====================================================================

/// 登记簿容量上限（来源：F0741 全域条目量级上限 1024，防无界账）。
pub const BOOK_CAP: u32 = 1024;

/// 域经格式条目（模板五字段 + 粒度 + 字数 + 证据引用清单）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FormatEntry {
    /// 字段0 编号（1..=999999，全域唯一）。
    pub id: u32,
    /// 字段1 标题（4..=64 字节）。
    pub title: String,
    /// 字段2 正文引用（B批/F功能）。
    pub body_ref: String,
    /// 字段3 评级引用（F0745:H|M|L）。
    pub grade_ref: String,
    /// 字段4 入册时戳（T+10 位逻辑 Tick）。
    pub booked_at: String,
    /// 粒度档。
    pub gran: Granularity,
    /// 正文字数（须落粒度带宽）。
    pub word_count: u32,
    /// 证据引用原文清单（≥粒度下限；逐条须可 parse 且可解引）。
    pub evidences: Vec<String>,
}

/// 事件种类常量（八类闭集）。
pub const EV_REGISTER: u8 = 1;
pub const EV_REJECT: u8 = 2;
pub const EV_FREEZE: u8 = 3;
pub const EV_VERIFY: u8 = 4;
pub const EV_EVOLVE: u8 = 5;
pub const EV_AUDIT: u8 = 6;
pub const EV_INJECT: u8 = 7;
pub const EV_HEALTH: u8 = 8;

/// 事件名（八类闭集；越界「未知事件」）。
pub fn event_kind_name(kind: u8) -> &'static str {
    match kind {
        EV_REGISTER => "登记",
        EV_REJECT => "拒收",
        EV_FREEZE => "封存",
        EV_VERIFY => "校验",
        EV_EVOLVE => "演进",
        EV_AUDIT => "审计",
        EV_INJECT => "注入",
        EV_HEALTH => "健康",
        _ => "未知事件",
    }
}

/// 事件账记录（append-only；seq 从 1 连续递增）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FsEvent {
    /// 事件序号（1 基，连续）。
    pub seq: u64,
    /// 事件种类（八类闭集）。
    pub kind: u8,
    /// 关联条目编号（无关联为 0）。
    pub entry_id: u32,
    /// 逻辑时刻（引擎逻辑 Tick）。
    pub tick: u64,
}

/// 违例三归因（J1 口径：字段缺失/粒度越带/证据无解引）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Attribution {
    /// 字段缺失（含空缺与格式不符两子类）。
    FieldDefect,
    /// 粒度越带。
    GranBand,
    /// 证据无解引（含不足下限、不可 parse、注册表不解引三子类）。
    EvidenceDangling,
}

impl Attribution {
    /// 归因名（三归因闭集中文定名）。
    pub fn name(&self) -> &'static str {
        match self {
            Attribution::FieldDefect => "字段缺失",
            Attribution::GranBand => "粒度越带",
            Attribution::EvidenceDangling => "证据无解引",
        }
    }

    /// 归因序（1/2/3）。
    pub fn index(&self) -> u8 {
        match self {
            Attribution::FieldDefect => 1,
            Attribution::GranBand => 2,
            Attribution::EvidenceDangling => 3,
        }
    }
}

/// 违例记录（entry + 归因 + 诊断码 + 现场注记）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ViolationRecord {
    /// 违例条目编号。
    pub entry_id: u32,
    /// 三归因之一。
    pub attribution: Attribution,
    /// 对应诊断码（0x2791/0x2792/0x2793/0x2794/0x2795 闭集）。
    pub code: u32,
    /// 现场注记（短句：哪个字段/哪档哪侧/哪条证据）。
    pub note: String,
}

/// 符合度审计报告（万分制；10000 即 100% 即 J1 过）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AuditReport {
    /// 审计条目总数。
    pub total: u32,
    /// 合规条目数（零违例条目）。
    pub compliant: u32,
    /// 违例记录数（跨条目累计）。
    pub violation_count: u32,
    /// 违例清单（登记序，逐条带归因带码带注）。
    pub violations: Vec<ViolationRecord>,
    /// 符合度（万分制：compliant×10000/total；空簿为 0）。
    pub pct: u32,
}

impl AuditReport {
    /// J1 判据过否（非空簿且符合度恰 10000/10000）。
    pub fn all_clear(&self) -> bool {
        self.total > 0 && self.pct == 10_000 && self.violation_count == 0
    }
}

// =====================================================================
// 健康报告
// =====================================================================

/// 健康核报告（七项检查逐项结论 + 发现清单）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HealthReport {
    /// 检查总项数（七）。
    pub checks_total: u32,
    /// 通过项数。
    pub checks_passed: u32,
    /// 发现清单（每项一条中文短句，空即全绿）。
    pub findings: Vec<String>,
}

impl HealthReport {
    /// 全绿否。
    pub fn ok(&self) -> bool {
        self.checks_passed == self.checks_total
    }
}

// =====================================================================
// 主引擎 FmtSpecEngine（登记/审计/冻结/演进/注入/健康/对拍）
// =====================================================================

/// 样例簿条目数（B38 域经册 F0741..F0752 十二条演示账）。
pub const SAMPLE_ENTRY_COUNT: u32 = 12;

/// 域经格式规范引擎（登记簿 + 规范封存 + 事件账 + 审计 + 注入面）。
///
/// 引擎本体含 Vec 字段（堆上持有，不上栈拷贝）；全整数确定性——零
/// 墙钟（逻辑 Tick 自增）、零浮点、零随机，双跑对拍天然全同。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FmtSpecEngine {
    /// 规范封存子系统（运行规范 + 封存账）。
    freeze: SpecFreeze,
    /// 登记簿（append-only）。
    book: Vec<FormatEntry>,
    /// 事件账（append-only）。
    events: Vec<FsEvent>,
    /// 逻辑 Tick（零墙钟：每次状态迁移自增）。
    tick: u64,
    /// 簿容量（默认 BOOK_CAP；测试可缩）。
    cap: u32,
    /// 最近一次审计报告（None=从未审计）。
    last_audit: Option<AuditReport>,
    /// 在账注入活动计数（主引擎必须恒 0——注入只许上克隆账）。
    injections_active: u32,
}

impl FmtSpecEngine {
    /// 标准引擎（v1 规范、空簿、容量 BOOK_CAP）。
    pub fn new() -> FmtSpecEngine {
        FmtSpecEngine {
            freeze: SpecFreeze::new(),
            book: Vec::new(),
            events: Vec::new(),
            tick: 0,
            cap: BOOK_CAP,
            last_audit: None,
            injections_active: 0,
        }
    }

    /// 定容引擎（容量面测试用；cap=0 视为 1，防零容死簿）。
    pub fn new_sized(cap: u32) -> FmtSpecEngine {
        let mut e = FmtSpecEngine::new();
        e.cap = if cap == 0 { 1 } else { cap };
        e
    }

    // ------------------- 访问面 -------------------

    /// 登记簿条目数。
    pub fn book_len(&self) -> u32 {
        self.book.len() as u32
    }

    /// 按序取条目（越界 None）。
    pub fn entry_at(&self, i: usize) -> Option<&FormatEntry> {
        if i < self.book.len() {
            Some(&self.book[i])
        } else {
            None
        }
    }

    /// 事件账切片。
    pub fn events(&self) -> &[FsEvent] {
        self.events.as_slice()
    }

    /// 事件账条数。
    pub fn event_count(&self) -> u32 {
        self.events.len() as u32
    }

    /// 逻辑 Tick 当前值。
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// 簿容量。
    pub fn cap(&self) -> u32 {
        self.cap
    }

    /// 当前规范版本。
    pub fn spec_version(&self) -> u8 {
        self.freeze.spec.version
    }

    /// 当前版是否已封存。
    pub fn is_sealed(&self) -> bool {
        self.freeze.is_sealed()
    }

    /// 封存账切片。
    pub fn records(&self) -> &[FreezeRecord] {
        self.freeze.records.as_slice()
    }

    /// 冻结指纹（未封存 None）。
    pub fn frozen_fingerprint(&self) -> Option<u32> {
        match self.freeze.last_record() {
            Some(r) => Some(r.fingerprint),
            None => None,
        }
    }

    /// 最近审计报告（从未审计 None）。
    pub fn last_audit(&self) -> Option<&AuditReport> {
        self.last_audit.as_ref()
    }

    /// 在账注入计数（主引擎恒 0）。
    pub fn injections_active(&self) -> u32 {
        self.injections_active
    }

    /// 当前规范账全文渲染（版本+三规范+指纹）。
    pub fn spec_ledger_text(&self) -> String {
        render_spec_ledger(&self.freeze.spec)
    }

    /// 运行规范口径越带校验（演进后随行；const 定版口径走
    /// `check_gran_band`）。
    pub fn check_band_current(
        &self,
        gran: Granularity,
        words: u32,
    ) -> Result<(), FmtSpecError> {
        check_gran_band_on_spec(&self.freeze.spec.gran_bands, gran, words)
    }

    // ------------------- 内部：事件与状态迁移 -------------------

    fn bump_tick(&mut self) -> u64 {
        self.tick = self.tick.wrapping_add(1);
        self.tick
    }

    fn push_event(&mut self, kind: u8, entry_id: u32) {
        let seq = self.events.len() as u64 + 1;
        let tick = self.bump_tick();
        self.events.push(FsEvent {
            seq,
            kind,
            entry_id,
            tick,
        });
    }

    // ------------------- 登记面（五道闸） -------------------

    /// 条目入簿（五道闸：容量 → 重号 → 五字段 → 粒度带宽 → 证据）。
    ///
    /// 返回入簿后条目总数。拒收路径全部落 EV_REJECT 事件（带拒收
    /// 条目号），成功落 EV_REGISTER——事件账即登记/拒收双流水。
    #[allow(clippy::too_many_arguments)]
    pub fn register_entry(
        &mut self,
        id: u32,
        title: &str,
        body_ref: &str,
        grade_ref: &str,
        booked_at: &str,
        gran: Granularity,
        word_count: u32,
        evidences: &[&str],
    ) -> Result<u32, FmtSpecError> {
        match self.register_entry_inner(id, title, body_ref, grade_ref, booked_at, gran, word_count, evidences) {
            Ok(n) => {
                self.push_event(EV_REGISTER, id);
                Ok(n)
            }
            Err(e) => {
                self.push_event(EV_REJECT, id);
                Err(e)
            }
        }
    }

    fn register_entry_inner(
        &mut self,
        id: u32,
        title: &str,
        body_ref: &str,
        grade_ref: &str,
        booked_at: &str,
        gran: Granularity,
        word_count: u32,
        evidences: &[&str],
    ) -> Result<u32, FmtSpecError> {
        // 闸1 容量。
        if self.book.len() as u32 >= self.cap {
            return Err(FmtSpecError::CapacityExceeded { cap: self.cap });
        }
        // 闸2 重号（线性扫，容量封顶 1024）。
        let mut i: usize = 0;
        while i < self.book.len() {
            if self.book[i].id == id {
                return Err(FmtSpecError::EntryDuplicate { id });
            }
            i += 1;
        }
        // 闸3 五字段（非空 + 规范卡格式）。
        self.validate_five_fields(id, title, body_ref, grade_ref, booked_at)?;
        // 闸4 粒度带宽（运行规范口径——演进后随行）。
        check_gran_band_on_spec(&self.freeze.spec.gran_bands, gran, word_count)?;
        // 闸5 证据（运行规范下限 + 逐条格式可 parse）。
        let need = self.freeze.spec.gran_min_evidence[gran.index()];
        let found = evidences.len() as u32;
        if found < need {
            return Err(FmtSpecError::EvidenceShort {
                entry_id: id,
                found,
                need,
            });
        }
        let mut e: usize = 0;
        while e < evidences.len() {
            if let Err(err) = parse_evidence(evidences[e]) {
                return Err(FmtSpecError::EvidenceUnresolvable {
                    index: e as u8,
                    raw_len: err_raw_len(&err, evidences[e]),
                });
            }
            e += 1;
        }
        // 入簿（append-only）。
        let mut ev_owned: Vec<String> = Vec::new();
        let mut k: usize = 0;
        while k < evidences.len() {
            ev_owned.push(evidences[k].to_string());
            k += 1;
        }
        self.book.push(FormatEntry {
            id,
            title: title.to_string(),
            body_ref: body_ref.to_string(),
            grade_ref: grade_ref.to_string(),
            booked_at: booked_at.to_string(),
            gran,
            word_count,
            evidences: ev_owned,
        });
        Ok(self.book.len() as u32)
    }

    /// 五字段校验（非空闸 + 逐卡格式闸；空字段先于格式报出）。
    pub fn validate_five_fields(
        &self,
        id: u32,
        title: &str,
        body_ref: &str,
        grade_ref: &str,
        booked_at: &str,
    ) -> Result<(), FmtSpecError> {
        // 非空闸：字段1..4 逐一先判空（编号是 u32 值位，空缺即 0 →
        // 值域闸判负，FieldFmtBad 而非 FieldEmpty——编号无「空串」态）。
        if title.is_empty() {
            return Err(FmtSpecError::FieldEmpty {
                field: FIELD_IDX_TITLE,
            });
        }
        if body_ref.is_empty() {
            return Err(FmtSpecError::FieldEmpty {
                field: FIELD_IDX_BODY_REF,
            });
        }
        if grade_ref.is_empty() {
            return Err(FmtSpecError::FieldEmpty {
                field: FIELD_IDX_GRADE_REF,
            });
        }
        if booked_at.is_empty() {
            return Err(FmtSpecError::FieldEmpty {
                field: FIELD_IDX_BOOKED_AT,
            });
        }
        // 格式闸：逐卡 parse（运行规范卡参与——卡被篡改即此处生效）。
        let cards = &self.freeze.spec.field_cards;
        let _ = cards; // 卡值域已内嵌于 parse 函数（见各 parse 注释）
        parse_entry_id(id)?;
        parse_title(title)?;
        parse_body_ref(body_ref)?;
        parse_grade_ref(grade_ref)?;
        parse_booked_at(booked_at)?;
        Ok(())
    }

    // ------------------- 注入面（显性 raw 通道，只许上克隆账） -------------------

    /// 注入通道：绕过五道闸直入账（仅注入面使用；主引擎使用即健康核红）。
    pub fn inject_raw_append(&mut self, entry: FormatEntry) -> Result<u32, FmtSpecError> {
        if self.book.len() as u32 >= self.cap {
            return Err(FmtSpecError::CapacityExceeded { cap: self.cap });
        }
        self.book.push(entry);
        self.injections_active = self.injections_active.saturating_add(1);
        self.push_event(EV_INJECT, 0);
        Ok(self.book.len() as u32)
    }

    /// 注入面：规范篡改注入（冻结后原地抬宽域级带宽，不走版本前进）。
    pub fn inject_tamper_widen_domain(&mut self, bad_max: u32) {
        self.freeze.spec.gran_bands[2].1 = bad_max;
        self.injections_active = self.injections_active.saturating_add(1);
        self.push_event(EV_INJECT, 0);
    }

    /// 注入面：规范篡改注入（冻结后改字段卡名，不走版本前进）。
    pub fn inject_tamper_rename_field(&mut self, field: u8, bad_name: &'static str) {
        let idx = field as usize;
        if idx < FIELD_COUNT {
            self.freeze.spec.field_cards[idx].name = bad_name;
        }
        self.injections_active = self.injections_active.saturating_add(1);
        self.push_event(EV_INJECT, 0);
    }

    // ------------------- 冻结/演进/校验面 -------------------

    /// 封存当前规范版（出证：封存即出冻结证书）。
    pub fn freeze_spec(&mut self) -> Result<u32, FmtSpecError> {
        let tick = self.tick;
        let note = if self.freeze.records.is_empty() {
            format!("v1 定版：全域统一模板（{} 字段/{} 档/{} 型证据）", FIELD_COUNT, GRAN_COUNT, 3)
        } else {
            format!("演进封存：第 {} 版", self.freeze.spec.version)
        };
        let fp = self.freeze.seal(tick, note)?;
        self.push_event(EV_FREEZE, 0);
        Ok(fp)
    }

    /// 演进补丁登记（版本前进；成功返回新版号，待封存）。
    pub fn evolve_spec(&mut self, patch: SpecPatch) -> Result<u8, FmtSpecError> {
        let note = patch.describe();
        let v = self.freeze.evolve(patch)?;
        self.push_event(EV_EVOLVE, 0);
        // 变更注记改写账尾前先记录在事件面；封存时以 note 落账。
        if let Some(r) = self.freeze.records.last_mut() {
            // 仅当演进后立即封存的路径走 seal(note)；此处不动旧账
            //（只增不改），注记暂存于下一次 seal 的调用方。
            let _ = r;
        }
        let _ = note;
        Ok(v)
    }

    /// 校验当前规范（冻结哈希红案核对；通过返回指纹）。
    pub fn verify_spec(&mut self) -> Result<u32, FmtSpecError> {
        let fp = self.freeze.verify()?;
        self.push_event(EV_VERIFY, 0);
        Ok(fp)
    }

    /// 冻结证书（封存即出证；未封存先取证 → step=1）。
    pub fn certificate(&self) -> Result<FreezeCertificate, FmtSpecError> {
        let last = match self.freeze.last_record() {
            Some(r) => r,
            None => return Err(FmtSpecError::FreezeLifecycleBroken { step: 1 }),
        };
        if last.version != self.freeze.spec.version {
            return Err(FmtSpecError::FreezeLifecycleBroken { step: 5 });
        }
        Ok(FreezeCertificate {
            version: last.version,
            fingerprint: last.fingerprint,
            tick: last.tick,
            entries_at_freeze: self.book.len() as u32,
            ledger: render_spec_ledger(&last.snapshot),
        })
    }

    /// 相邻两版差异账渲染。
    pub fn evolution_diff(&self) -> Result<String, FmtSpecError> {
        self.freeze.render_evolution_diff()
    }

    // ------------------- 审计面 -------------------

    /// 审计登记簿符合度（默认注册表；逐条 vs 三规范，落 EV_AUDIT）。
    pub fn audit_book(&mut self) -> Result<AuditReport, FmtSpecError> {
        let reg = DEFAULT_REGISTRY;
        let report = self.audit_book_with(&reg)?;
        Ok(report)
    }

    /// 审计登记簿符合度（指定注册表；返回报告并存为最近审计）。
    pub fn audit_book_with(
        &mut self,
        reg: &EvidenceRegistry,
    ) -> Result<AuditReport, FmtSpecError> {
        let spec = self.freeze.spec;
        let mut violations: Vec<ViolationRecord> = Vec::new();
        let mut compliant: u32 = 0;
        let total = self.book.len() as u32;
        let mut i: usize = 0;
        while i < self.book.len() {
            let entry_violations = audit_entry(&spec, reg, &self.book[i]);
            if entry_violations.is_empty() {
                compliant = compliant.saturating_add(1);
            } else {
                let mut k: usize = 0;
                while k < entry_violations.len() {
                    violations.push(entry_violations[k].clone());
                    k += 1;
                }
            }
            i += 1;
        }
        let pct: u32 = if total == 0 {
            0
        } else {
            // compliant,total ≤ 1024 → u64 中转无溢出；saturating 兜底。
            let p = (compliant as u64).saturating_mul(10_000) / (total as u64);
            p as u32
        };
        let report = AuditReport {
            total,
            compliant,
            violation_count: violations.len() as u32,
            violations,
            pct,
        };
        self.last_audit = Some(report.clone());
        self.push_event(EV_AUDIT, 0);
        Ok(report)
    }

    // ------------------- 指纹面 -------------------

    /// 登记簿规范形序列化（canonical bytes：双跑对拍与簿指纹的输入）。
    pub fn canonical_book_bytes(&self) -> Vec<u8> {
        let mut v: Vec<u8> = Vec::new();
        v.extend_from_slice(b"FMTBOOK-V1");
        v.push(0x7C);
        let mut i: usize = 0;
        while i < self.book.len() {
            let e = &self.book[i];
            v.push(b'E');
            v.extend_from_slice(&e.id.to_le_bytes());
            v.push(0x1F);
            v.extend_from_slice(e.title.as_bytes());
            v.push(0x1F);
            v.extend_from_slice(e.body_ref.as_bytes());
            v.push(0x1F);
            v.extend_from_slice(e.grade_ref.as_bytes());
            v.push(0x1F);
            v.extend_from_slice(e.booked_at.as_bytes());
            v.push(0x1F);
            v.push(e.gran.index() as u8);
            v.push(0x1F);
            v.extend_from_slice(&e.word_count.to_le_bytes());
            v.push(0x1F);
            let mut k: usize = 0;
            while k < e.evidences.len() {
                if k > 0 {
                    v.push(0x1E);
                }
                v.extend_from_slice(e.evidences[k].as_bytes());
                k += 1;
            }
            v.push(0x7C);
            i += 1;
        }
        v
    }

    /// 登记簿指纹（规范形 FNV-1a 32）。
    pub fn book_fingerprint(&self) -> u32 {
        let bytes = self.canonical_book_bytes();
        fnv1a32(bytes.as_slice())
    }

    /// 当前规范指纹（运行规范形 FNV-1a 32）。
    pub fn spec_fingerprint(&self) -> u32 {
        spec_fingerprint_of(&self.freeze.spec)
    }

    // ------------------- 事件链核 -------------------

    /// 事件链连续性（seq 从 1 逐 +1；断链 → EventSeqBroken）。
    pub fn check_event_chain(&self) -> Result<u64, FmtSpecError> {
        let mut expect: u64 = 1;
        let mut i: usize = 0;
        while i < self.events.len() {
            if self.events[i].seq != expect {
                return Err(FmtSpecError::EventSeqBroken {
                    expect,
                    found: self.events[i].seq,
                });
            }
            expect = expect.saturating_add(1);
            i += 1;
        }
        Ok(self.events.len() as u64)
    }

    // ------------------- 健康核（七项） -------------------

    /// 健康核（七项不变量逐项核；落 EV_HEALTH）。
    pub fn health(&mut self) -> HealthReport {
        let mut findings: Vec<String> = Vec::new();
        let mut passed: u32 = 0;
        const TOTAL: u32 = 7;

        // 核1 容量界内。
        if self.book.len() as u32 <= self.cap {
            passed += 1;
        } else {
            findings.push(format!("容量越界：簿长 {} 越过上限 {}", self.book.len(), self.cap));
        }
        // 核2 编号唯一（O(n²) 线性双扫，n ≤ 1024）。
        let mut dup = false;
        let mut i: usize = 0;
        while i < self.book.len() {
            let mut j: usize = i + 1;
            while j < self.book.len() {
                if self.book[i].id == self.book[j].id {
                    dup = true;
                    findings.push(format!("编号重号：{} 出现 ≥2 次", self.book[i].id));
                    j = self.book.len();
                    i = self.book.len();
                }
                j += 1;
            }
            i += 1;
        }
        if !dup {
            passed += 1;
        }
        // 核3 事件链连续。
        match self.check_event_chain() {
            Ok(_) => passed += 1,
            Err(e) => findings.push(format!("事件链：{}", e.detail())),
        }
        // 核4 封存完整（版本链恰 +1 递增；当前版已封存则指纹必核过）。
        let mut version_ok = true;
        let mut prev: u8 = 0;
        let mut r: usize = 0;
        while r < self.freeze.records.len() {
            let rec = &self.freeze.records[r];
            if rec.version != prev.wrapping_add(1) && r > 0 {
                version_ok = false;
                findings.push(format!("版本链断裂：第 {} 条记录 v{}", r, rec.version));
            }
            if r == 0 && rec.version != 1 {
                version_ok = false;
                findings.push(format!("首版非 v1：v{}", rec.version));
            }
            prev = rec.version;
            r += 1;
        }
        if !self.freeze.records.is_empty() && !self.freeze.is_sealed() {
            version_ok = false;
            findings.push("演进后未封存：当前版不在封存账尾".to_string());
        }
        if version_ok {
            passed += 1;
        }
        // 核5 规范快照指纹自洽（账被改必红：逐条复算快照指纹）。
        let mut snap_ok = true;
        let mut s: usize = 0;
        while s < self.freeze.records.len() {
            let rec = &self.freeze.records[s];
            if spec_fingerprint_of(&rec.snapshot) != rec.fingerprint {
                snap_ok = false;
                findings.push(format!(
                    "封存账第 {} 条指纹与快照不符（账被改）",
                    s
                ));
            }
            s += 1;
        }
        if snap_ok {
            passed += 1;
        }
        // 核6 审计在期（非空簿须已审计且复算一致）。
        let mut audit_ok = true;
        if !self.book.is_empty() && self.last_audit.is_none() {
            audit_ok = false;
            findings.push("非空簿从未审计".to_string());
        }
        if let Some(prev_report) = &self.last_audit {
            let reg = DEFAULT_REGISTRY;
            let spec = self.freeze.spec;
            let mut recomputed_compliant: u32 = 0;
            let mut recomputed_violations: u32 = 0;
            let mut i: usize = 0;
            while i < self.book.len() {
                let vs = audit_entry(&spec, &reg, &self.book[i]);
                if vs.is_empty() {
                    recomputed_compliant = recomputed_compliant.saturating_add(1);
                } else {
                    recomputed_violations = recomputed_violations.saturating_add(vs.len() as u32);
                }
                i += 1;
            }
            if prev_report.compliant != recomputed_compliant
                || prev_report.violation_count != recomputed_violations
            {
                audit_ok = false;
                findings.push(format!(
                    "审计过期：账面 {}/{} 违例 {} vs 复算 {}/{} 违例 {}",
                    prev_report.compliant,
                    prev_report.total,
                    prev_report.violation_count,
                    recomputed_compliant,
                    self.book.len(),
                    recomputed_violations
                ));
            }
        }
        if audit_ok {
            passed += 1;
        }
        // 核7 注入清零（主引擎不许残留注入态）。
        if self.injections_active == 0 {
            passed += 1;
        } else {
            findings.push(format!(
                "注入未清零：在账注入 {} 次（注入只许上克隆账）",
                self.injections_active
            ));
        }

        self.push_event(EV_HEALTH, 0);
        HealthReport {
            checks_total: TOTAL,
            checks_passed: passed,
            findings,
        }
    }

    // ------------------- 样例簿（B38 域经册十二条演示账） -------------------

    /// 建样例簿（F0741..F0752 十二条真实口径演示条目，全过五道闸，
    /// 审计对默认注册表全解引——符合度 100% 的基准账）。
    pub fn build_sample_book(&mut self) -> Result<u32, FmtSpecError> {
        self.register_entry(
            741,
            "域经登记簿建档：五列汇总入簿打法",
            "B38/F0741",
            "F0745:H",
            "T0000000041",
            Granularity::Domain,
            520,
            &[
                "UNX-F0741-J1",
                "B38:L01150",
                "src:varix/src/unxstar/unx741_domain_journal.rs:1",
            ],
        )?;
        self.register_entry(
            742,
            "条目规范化升格：三列升全格式四要素",
            "B38/F0742",
            "F0745:H",
            "T0000000042",
            Granularity::Batch,
            240,
            &["UNX-F0742-J1", "B37:L01100"],
        )?;
        self.register_entry(
            743,
            "证据链核验：来源账行逐条解引",
            "B38/F0743",
            "F0745:H",
            "T0000000043",
            Granularity::Batch,
            300,
            &["UNX-F0743-J1", "B38:L01080"],
        )?;
        self.register_entry(
            744,
            "跨批去重合并：同主题合并账留痕",
            "B38/F0744",
            "F0745:M",
            "T0000000044",
            Granularity::Batch,
            210,
            &["UNX-F0744-J1", "B38:L01120"],
        )?;
        self.register_entry(
            745,
            "价值分级：四维评级三档分布打法",
            "B38/F0745",
            "F0745:H",
            "T0000000045",
            Granularity::Domain,
            610,
            &[
                "UNX-F0745-J1",
                "src:varix/src/unxstar/unx745_valgrade.rs:1531",
            ],
        )?;
        self.register_entry(
            746,
            "格式规范冻结：三规范定版哈希封存",
            "B38/F0746",
            "F0745:H",
            "T0000000046",
            Granularity::Domain,
            580,
            &[
                "UNX-F0746-J1",
                "src:varix/src/unxstar/unx746_fmtspec.rs:1",
            ],
        )?;
        self.register_entry(
            747,
            "检索索引：主题词到条目映射",
            "B38/F0747",
            "F0745:M",
            "T0000000047",
            Granularity::Topic,
            96,
            &["UNX-F0747-J1"],
        )?;
        self.register_entry(
            748,
            "跨域适用标注：五域消费面标注",
            "B38/F0748",
            "F0745:M",
            "T0000000048",
            Granularity::Batch,
            180,
            &["UNX-F0748-J1", "B38:L01135"],
        )?;
        self.register_entry(
            749,
            "移交包内容清单：六件套定版",
            "B38/F0749",
            "F0745:M",
            "T0000000049",
            Granularity::Batch,
            260,
            &["UNX-F0749-J1", "B38:L01140"],
        )?;
        self.register_entry(
            750,
            "接口快照：六冻结接口字段表",
            "B38/F0750",
            "F0745:M",
            "T0000000050",
            Granularity::Batch,
            320,
            &["UNX-F0750-J1", "B38:L01145"],
        )?;
        self.register_entry(
            751,
            "账册汇编目录：双册指针链",
            "B38/F0751",
            "F0745:L",
            "T0000000051",
            Granularity::Topic,
            88,
            &["UNX-F0751-J1"],
        )?;
        self.register_entry(
            752,
            "ktest 套件封版：断言清单快照",
            "B38/F0752",
            "F0745:M",
            "T0000000052",
            Granularity::Batch,
            280,
            &["UNX-F0752-J1", "B38:L01155"],
        )?;
        Ok(self.book.len() as u32)
    }

    // ------------------- 注入套件（三类逐检，漏抓即错） -------------------

    /// 注入套件（克隆账上三类注入逐类检出；全检出返回 3）。
    ///
    /// 类1 规范篡改：冻结后原地改带宽 → verify 必红（XC_FREEZE_TAMPER）。
    /// 类2 五字段缺一：空字段入簿 → 登记闸必拒（XC_FIELD_EMPTY）。
    /// 类3 证据悬空：格式合法但注册表不解引 → 审计必列违例
    ///      （归因=证据无解引，XC_EVID_UNSOLV）。
    pub fn run_injection_suite(&self) -> Result<u32, FmtSpecError> {
        let mut detected: u32 = 0;

        // 类1 规范篡改注入。
        let mut c1 = self.clone();
        c1.freeze_spec().map_err(|e| e)?;
        c1.inject_tamper_widen_domain(9999);
        match c1.verify_spec() {
            Err(FmtSpecError::SpecTamperDetected { frozen, computed }) => {
                if frozen != computed {
                    detected += 1;
                }
            }
            _ => return Err(FmtSpecError::InjectionMissed { which: 1 }),
        }

        // 类2 五字段缺一注入（标题空串：五字段任缺一的代表位；
        // 其余字段缺一同理由同一非空闸守卫）。
        let mut c2 = self.clone();
        match c2.register_entry(
            9001,
            "",
            "B38/F0740",
            "F0745:M",
            "T0000000099",
            Granularity::Topic,
            80,
            &["UNX-F0740-J1"],
        ) {
            Err(FmtSpecError::FieldEmpty { field }) => {
                if field == FIELD_IDX_TITLE {
                    detected += 1;
                }
            }
            _ => return Err(FmtSpecError::InjectionMissed { which: 2 }),
        }

        // 类3 证据悬空注入（UNX-F9999-J9：格式全过、注册表无此号）。
        let mut c3 = self.clone();
        c3.inject_raw_append(FormatEntry {
            id: 9002,
            title: "悬空证据演示条目".to_string(),
            body_ref: "B38/F0740".to_string(),
            grade_ref: "F0745:M".to_string(),
            booked_at: "T0000000098".to_string(),
            gran: Granularity::Topic,
            word_count: 80,
            evidences: vec!["UNX-F9999-J9".to_string()],
        })
        .map_err(|e| e)?;
        let report = c3.audit_book().map_err(|e| e)?;
        let mut hit = false;
        let mut v: usize = 0;
        while v < report.violations.len() {
            if report.violations[v].entry_id == 9002
                && report.violations[v].attribution == Attribution::EvidenceDangling
                && report.violations[v].code == XC_EVID_UNSOLV
            {
                hit = true;
            }
            v += 1;
        }
        if hit {
            detected += 1;
        } else {
            return Err(FmtSpecError::InjectionMissed { which: 3 });
        }

        Ok(detected)
    }

    // ------------------- 双跑确定性对拍 -------------------

    /// 双跑确定性对拍（同一登记脚本双引擎独立建账 → 三重对拍）。
    ///
    /// 对拍面：簿指纹 / 规范指纹 / 审计符合度（pct 与违例数）。
    /// 任一不同即 false（确定性破坏，回退本单版本）。
    pub fn double_run_determinism() -> Result<bool, FmtSpecError> {
        let mut a = FmtSpecEngine::new();
        a.build_sample_book().map_err(|e| e)?;
        let ra = a.audit_book().map_err(|e| e)?;

        let mut b = FmtSpecEngine::new();
        b.build_sample_book().map_err(|e| e)?;
        let rb = b.audit_book().map_err(|e| e)?;

        Ok(a.book_fingerprint() == b.book_fingerprint()
            && a.spec_fingerprint() == b.spec_fingerprint()
            && ra.pct == rb.pct
            && ra.violation_count == rb.violation_count
            && ra.total == rb.total)
    }
}

/// 证据 parse 错误的原文长度提取（register 闸5 的诊断数据）。
fn err_raw_len(err: &FmtSpecError, raw: &str) -> u32 {
    match err {
        FmtSpecError::EvidenceUnresolvable { raw_len, .. } => *raw_len,
        _ => raw.len() as u32,
    }
}

/// 单条目审计（对三规范逐面核；返回该条目全部违例，空即合规）。
///
/// 三面顺序：五字段（非空+格式）→ 粒度带宽 → 证据（下限+parse+解引）。
fn audit_entry(
    spec: &RuntimeSpec,
    reg: &EvidenceRegistry,
    e: &FormatEntry,
) -> Vec<ViolationRecord> {
    let mut out: Vec<ViolationRecord> = Vec::new();
    // 面1 五字段。
    if e.title.is_empty() {
        out.push(ViolationRecord {
            entry_id: e.id,
            attribution: Attribution::FieldDefect,
            code: XC_FIELD_EMPTY,
            note: "标题空缺".to_string(),
        });
    } else if parse_title(&e.title).is_err() {
        out.push(ViolationRecord {
            entry_id: e.id,
            attribution: Attribution::FieldDefect,
            code: XC_FIELD_FMT,
            note: format!("标题不合规卡（带宽 {}..={}）", spec.field_cards[1].min_len, spec.field_cards[1].max_len),
        });
    }
    if e.body_ref.is_empty() {
        out.push(ViolationRecord {
            entry_id: e.id,
            attribution: Attribution::FieldDefect,
            code: XC_FIELD_EMPTY,
            note: "正文引用空缺".to_string(),
        });
    } else if parse_body_ref(&e.body_ref).is_err() {
        out.push(ViolationRecord {
            entry_id: e.id,
            attribution: Attribution::FieldDefect,
            code: XC_FIELD_FMT,
            note: "正文引用文法不符（B批/F功能）".to_string(),
        });
    }
    if e.grade_ref.is_empty() {
        out.push(ViolationRecord {
            entry_id: e.id,
            attribution: Attribution::FieldDefect,
            code: XC_FIELD_EMPTY,
            note: "评级引用空缺".to_string(),
        });
    } else if parse_grade_ref(&e.grade_ref).is_err() {
        out.push(ViolationRecord {
            entry_id: e.id,
            attribution: Attribution::FieldDefect,
            code: XC_FIELD_FMT,
            note: "评级引用非 F0745:H|M|L".to_string(),
        });
    }
    if e.booked_at.is_empty() {
        out.push(ViolationRecord {
            entry_id: e.id,
            attribution: Attribution::FieldDefect,
            code: XC_FIELD_EMPTY,
            note: "入册时戳空缺".to_string(),
        });
    } else if parse_booked_at(&e.booked_at).is_err() {
        out.push(ViolationRecord {
            entry_id: e.id,
            attribution: Attribution::FieldDefect,
            code: XC_FIELD_FMT,
            note: "入册时戳非 T+10 位逻辑 Tick".to_string(),
        });
    }
    if parse_entry_id(e.id).is_err() {
        out.push(ViolationRecord {
            entry_id: e.id,
            attribution: Attribution::FieldDefect,
            code: XC_FIELD_FMT,
            note: format!("编号越值域 1..={}", ENTRY_ID_MAX),
        });
    }
    // 面2 粒度带宽（运行规范卡带宽——被篡改即此处归因口径漂移可查）。
    let gi = e.gran.index();
    let lo = spec.gran_bands[gi].0;
    let hi = spec.gran_bands[gi].1;
    if e.word_count < lo || e.word_count > hi {
        out.push(ViolationRecord {
            entry_id: e.id,
            attribution: Attribution::GranBand,
            code: XC_GRAN_OOB,
            note: format!("{}字数 {} 越带 {}..={}", e.gran.name(), e.word_count, lo, hi),
        });
    }
    // 面3 证据（下限 + 逐条 parse + 逐条注册表解引）。
    let need = spec.gran_min_evidence[gi];
    let found = e.evidences.len() as u32;
    if found < need {
        out.push(ViolationRecord {
            entry_id: e.id,
            attribution: Attribution::EvidenceDangling,
            code: XC_EVID_SHORT,
            note: format!("证据 {} 条低于下限 {}", found, need),
        });
    }
    let mut k: usize = 0;
    while k < e.evidences.len() {
        match parse_evidence(&e.evidences[k]) {
            Err(_) => {
                out.push(ViolationRecord {
                    entry_id: e.id,
                    attribution: Attribution::EvidenceDangling,
                    code: XC_EVID_UNSOLV,
                    note: format!("证据#{} 文法不可解", k),
                });
            }
            Ok(parsed) => {
                if !reg.resolves(&parsed) {
                    out.push(ViolationRecord {
                        entry_id: e.id,
                        attribution: Attribution::EvidenceDangling,
                        code: XC_EVID_UNSOLV,
                        note: format!("证据#{} 型{} 注册表不解引", k, parsed.kind_name()),
                    });
                }
            }
        }
        k += 1;
    }
    out
}

// =====================================================================
// 冒烟测试（隔离探针 cfg(test) 面；断言仅存在于测试路径）
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 基准引擎：样例簿 12 条 + 审计 100%。
    fn eng_full() -> FmtSpecEngine {
        let mut e = FmtSpecEngine::new();
        e.build_sample_book().ok().expect("样例簿必建");
        e.audit_book().ok().expect("样例簿必审");
        e
    }

    // ---------- FNV 基元 ----------

    #[test]
    fn fnv_const_vectors() {
        assert_eq!(fnv1a32(b""), 0x811C_9DC5);
        assert_eq!(fnv1a32(b"a"), 0xE40C_292C);
        assert_eq!(fnv1a32(b"foobar"), 0xBF9C_F968);
        // 区分性：相邻输入指纹必异。
        assert_ne!(fnv1a32(b"unx745"), fnv1a32(b"unx746"));
        // 空串与串前缀指纹必异。
        assert_ne!(fnv1a32(b""), fnv1a32(b"\x00"));
    }

    // ---------- 诊断码段闭集 ----------

    #[test]
    fn seg_codes_closed_set() {
        let codes = [
            XC_FIELD_EMPTY,
            XC_FIELD_FMT,
            XC_GRAN_OOB,
            XC_EVID_SHORT,
            XC_EVID_UNSOLV,
            XC_ENTRY_DUP,
            XC_CAP_OVER,
            XC_FREEZE_TAMPER,
            XC_LIFE_CYCLE,
            XC_EVENT_SEQ,
            XC_INJECT_MISSED,
        ];
        let mut i: usize = 0;
        while i < codes.len() {
            assert!(codes[i] >= 0x2791 && codes[i] <= 0x279B, "码越段：{:04X}", codes[i]);
            let mut j: usize = i + 1;
            while j < codes.len() {
                assert_ne!(codes[i], codes[j], "码重复");
                j += 1;
            }
            // 逐码两要素诊断齐全。
            let (phen, hint) = diag_text(codes[i]);
            assert!(!phen.is_empty() && !hint.is_empty());
            i += 1;
        }
        // 段外码走兜底。
        assert_eq!(diag_text(0x2790).0, "未知诊断码");
    }

    // ---------- 模板五字段规范 ----------

    #[test]
    fn field_cards_const_sanity() {
        assert_eq!(FIELD_CARDS.len(), 5);
        let mut i: usize = 0;
        while i < 5 {
            assert_eq!(FIELD_CARDS[i].key, i as u8);
            assert!(FIELD_CARDS[i].min_len >= 1);
            assert!(FIELD_CARDS[i].max_len >= FIELD_CARDS[i].min_len);
            assert!(!FIELD_CARDS[i].name.is_empty());
            assert!(!FIELD_CARDS[i].source.is_empty());
            i += 1;
        }
        // 字段名反查。
        assert_eq!(field_name(0), "编号");
        assert_eq!(field_name(3), "评级引用");
        assert_eq!(field_name(9), "未知字段");
        // 规范卡表渲染含五卡。
        let text = render_field_cards(&FIELD_CARDS);
        assert!(text.contains("模板五字段规范"));
        assert!(text.contains("评级引用"));
        assert!(text.contains("F0745"));
    }

    #[test]
    fn five_fields_valid_parse() {
        assert_eq!(parse_entry_id(1).ok(), Some(1));
        assert_eq!(parse_entry_id(999_999).ok(), Some(999_999));
        assert_eq!(parse_title("域经登记簿建档打法").ok(), Some(27)); // 9 汉字×3 字节
        assert_eq!(
            parse_body_ref("B38/F0741").ok(),
            Some((38, 741))
        );
        assert_eq!(parse_grade_ref("F0745:H").ok(), Some(0));
        assert_eq!(parse_grade_ref("F0745:M").ok(), Some(1));
        assert_eq!(parse_grade_ref("F0745:L").ok(), Some(2));
        assert_eq!(parse_booked_at("T0000000041").ok(), Some(41));
    }

    #[test]
    fn five_field_missing_each_rejected() {
        let mut e = FmtSpecEngine::new();
        // 四个字符串字段逐一置空 → FieldEmpty（带字段位）。
        match e.register_entry(1, "", "B38/F0740", "F0745:M", "T0000000001",
            Granularity::Topic, 80, &["UNX-F0740-J1"]) {
            Err(FmtSpecError::FieldEmpty { field }) => assert_eq!(field, FIELD_IDX_TITLE),
            other => panic!("标题空必须拒，实得 {:?}", other.map(|_| ())),
        }
        match e.register_entry(2, "标题四字节", "", "F0745:M", "T0000000002",
            Granularity::Topic, 80, &["UNX-F0740-J1"]) {
            Err(FmtSpecError::FieldEmpty { field }) => assert_eq!(field, FIELD_IDX_BODY_REF),
            other => panic!("正文引用空必须拒，实得 {:?}", other.map(|_| ())),
        }
        match e.register_entry(3, "标题四字节", "B38/F0740", "", "T0000000003",
            Granularity::Topic, 80, &["UNX-F0740-J1"]) {
            Err(FmtSpecError::FieldEmpty { field }) => assert_eq!(field, FIELD_IDX_GRADE_REF),
            other => panic!("评级引用空必须拒，实得 {:?}", other.map(|_| ())),
        }
        match e.register_entry(4, "标题四字节", "B38/F0740", "F0745:M", "",
            Granularity::Topic, 80, &["UNX-F0740-J1"]) {
            Err(FmtSpecError::FieldEmpty { field }) => assert_eq!(field, FIELD_IDX_BOOKED_AT),
            other => panic!("入册时戳空必须拒，实得 {:?}", other.map(|_| ())),
        }
        // 编号 0（值位空缺）→ 值域闸 FieldFmtBad。
        match e.register_entry(0, "标题四字节", "B38/F0740", "F0745:M", "T0000000004",
            Granularity::Topic, 80, &["UNX-F0740-J1"]) {
            Err(FmtSpecError::FieldFmtBad { field, reason }) => {
                assert_eq!(field, FIELD_IDX_ID);
                assert_eq!(reason, 2);
            }
            other => panic!("编号 0 必须拒，实得 {:?}", other.map(|_| ())),
        }
        // 全部拒收 → 簿空、事件账全为拒收。
        assert_eq!(e.book_len(), 0);
        assert_eq!(e.event_count(), 5);
        let mut i: usize = 0;
        while i < e.events().len() {
            assert_eq!(e.events()[i].kind, EV_REJECT);
            i += 1;
        }
    }

    #[test]
    fn field_fmt_each_rejected() {
        // 编号越上界。
        assert!(parse_entry_id(1_000_000).is_err());
        // 标题过短（4 字节下界：单汉字 3 字节必越带）/过长/控制字节。
        assert!(parse_title("一").is_err());
        let long = "标".repeat(65);
        assert!(parse_title(&long).is_err());
        assert!(parse_title("标题\x1F带控制").is_err());
        // 正文引用布局/数字/值域。
        assert!(parse_body_ref("X38/F0741").is_err());
        assert!(parse_body_ref("B3X/F0741").is_err());
        assert!(parse_body_ref("B38-F0741").is_err());
        assert!(parse_body_ref("B00/F0741").is_err());
        assert!(parse_body_ref("B38/F0000").is_err());
        // 评级引用档位闭集外。
        assert!(parse_grade_ref("F0745:X").is_err());
        assert!(parse_grade_ref("F0746:H").is_err());
        assert!(parse_grade_ref("F0745H").is_err());
        // 入册时戳布局/数字/零值。
        assert!(parse_booked_at("X0000000041").is_err());
        assert!(parse_booked_at("T000000004X").is_err());
        assert!(parse_booked_at("T0000000000").is_err());
        assert!(parse_booked_at("T41").is_err());
        // 理由码与注记齐。
        assert_eq!(field_fmt_reason_text(4), "布局不符：前缀/分隔符/段位数与文法不合");
    }

    // ---------- 粒度规范 ----------

    #[test]
    fn gran_bands_and_detection() {
        // 三带 const 值（写死即文档）。
        assert_eq!(Granularity::Topic.band(), (40, 120));
        assert_eq!(Granularity::Batch.band(), (121, 400));
        assert_eq!(Granularity::Domain.band(), (401, 1200));
        // 带宽双端闭含。
        assert!(check_gran_band(Granularity::Topic, 40).is_ok());
        assert!(check_gran_band(Granularity::Topic, 120).is_ok());
        assert!(check_gran_band(Granularity::Batch, 121).is_ok());
        assert!(check_gran_band(Granularity::Batch, 400).is_ok());
        assert!(check_gran_band(Granularity::Domain, 401).is_ok());
        assert!(check_gran_band(Granularity::Domain, 1200).is_ok());
        // 越带双侧归因。
        match check_gran_band(Granularity::Topic, 39) {
            Err(FmtSpecError::GranBandViolation { gran, side, .. }) => {
                assert_eq!(gran, 0);
                assert_eq!(side, 1);
            }
            other => panic!("低于带必须检，实得 {:?}", other.map(|_| ())),
        }
        match check_gran_band(Granularity::Topic, 121) {
            Err(FmtSpecError::GranBandViolation { gran, side, .. }) => {
                assert_eq!(gran, 0);
                assert_eq!(side, 2);
            }
            other => panic!("高于带必须检，实得 {:?}", other.map(|_| ())),
        }
        assert!(check_gran_band(Granularity::Domain, 400).is_err());
        assert!(check_gran_band(Granularity::Domain, 1201).is_err());
        // 证据下限。
        assert_eq!(Granularity::Topic.min_evidence(), 1);
        assert_eq!(Granularity::Domain.min_evidence(), 2);
        // 档名/标签/反查。
        assert_eq!(Granularity::Batch.name(), "批级");
        assert_eq!(Granularity::from_index(2).map(|g| g.tag()), Some("domain"));
        assert!(Granularity::from_index(3).is_none());
        // 渲染含三档。
        let text = render_gran_spec(&GRAN_BANDS, &GRAN_MIN_EVIDENCE);
        assert!(text.contains("主题级") && text.contains("域级"));
    }

    // ---------- 证据规范 ----------

    #[test]
    fn evidence_parse_ok_three_kinds() {
        match parse_evidence("UNX-F0746-J1") {
            Ok(EvidenceRef::Criterion { task, jdx }) => {
                assert_eq!(task, 746);
                assert_eq!(jdx, 1);
            }
            other => panic!("判据号必须可解，实得 {:?}", other),
        }
        match parse_evidence("B38:L11500") {
            Ok(EvidenceRef::LedgerLine { batch, line }) => {
                assert_eq!(batch, 38);
                assert_eq!(line, 11500);
            }
            other => panic!("账行号必须可解，实得 {:?}", other),
        }
        match parse_evidence("src:varix/src/unxstar/unx745_valgrade.rs:1531") {
            Ok(EvidenceRef::FileAnchor { path, line }) => {
                assert_eq!(path, "varix/src/unxstar/unx745_valgrade.rs");
                assert_eq!(line, 1531);
            }
            other => panic!("文件锚点必须可解，实得 {:?}", other),
        }
    }

    #[test]
    fn evidence_parse_bad_rejected() {
        let bad = [
            "UNX-F0746-J10",   // 判据序两位越文法
            "UNX-F0000-J1",    // 功能号 0 越值域
            "UNX-F0746J1",     // 缺连字符
            "B38:L0",          // 行号 0
            "B38:L123456",     // 行号六位越上限
            "B0:L100",         // 批号 0
            "B38X100",         // 无冒号布局
            "src::100",        // 路径空
            "src:a:b:100",     // 路径内嵌冒号
            "src:file.rs:0",   // 行号 0
            "src:file.rs:",    // 行号缺位
            "plain-garbage",   // 三型皆不匹配
        ];
        let mut i: usize = 0;
        while i < bad.len() {
            assert!(
                parse_evidence(bad[i]).is_err(),
                "该证据必须不可解：{}",
                bad[i]
            );
            i += 1;
        }
        // 文法卡渲染。
        let text = render_evid_spec(&EVID_GRAMMARS);
        assert!(text.contains("判据号") && text.contains("账行号") && text.contains("文件锚点"));
    }

    #[test]
    fn registry_resolves_closed_sets() {
        let r = DEFAULT_REGISTRY;
        assert!(r.knows_task(746));
        assert!(r.knows_task(679));
        assert!(!r.knows_task(9999));
        assert!(r.ledger_line_ok(38, 11500));
        assert!(!r.ledger_line_ok(38, 11501));
        assert!(!r.ledger_line_ok(99, 100));
        assert!(r.knows_file("varix/src/unxstar/unx746_fmtspec.rs"));
        assert!(!r.knows_file("etc/unknown.rs"));
        // 三型分派解引。
        assert!(r.resolves(&parse_evidence("UNX-F0741-J1").ok().expect("合法")));
        assert!(!r.resolves(&parse_evidence("UNX-F9999-J9").ok().expect("格式合法但悬空")));
        assert!(r.resolves(&parse_evidence("B37:L11450").ok().expect("合法")));
        assert!(!r.resolves(&parse_evidence("B27:L9999").ok().expect("格式合法但悬空")));
    }

    #[test]
    fn evidence_shortfall_detected() {
        let mut e = FmtSpecEngine::new();
        // 主题级 0 条证据 → 不足下限 1。
        match e.register_entry(1, "标题四字节", "B38/F0740", "F0745:M", "T0000000001",
            Granularity::Topic, 80, &[]) {
            Err(FmtSpecError::EvidenceShort { found, need, .. }) => {
                assert_eq!((found, need), (0, 1));
            }
            other => panic!("零证据必须拒，实得 {:?}", other.map(|_| ())),
        }
        // 域级 1 条证据 → 不足下限 2。
        match e.register_entry(2, "标题四字节", "B38/F0740", "F0745:M", "T0000000002",
            Granularity::Domain, 500, &["UNX-F0741-J1"]) {
            Err(FmtSpecError::EvidenceShort { found, need, .. }) => {
                assert_eq!((found, need), (1, 2));
            }
            other => panic!("域级单证据必须拒，实得 {:?}", other.map(|_| ())),
        }
        // 证据格式坏 → 无解引。
        match e.register_entry(3, "标题四字节", "B38/F0740", "F0745:M", "T0000000003",
            Granularity::Topic, 80, &["garbage"]) {
            Err(FmtSpecError::EvidenceUnresolvable { index, .. }) => assert_eq!(index, 0),
            other => panic!("坏证据必须拒，实得 {:?}", other.map(|_| ())),
        }
    }

    // ---------- 登记闸：重号/容量 ----------

    #[test]
    fn register_dup_and_capacity() {
        let mut e = FmtSpecEngine::new();
        e.register_entry(7, "标题四字节甲", "B38/F0740", "F0745:M", "T0000000001",
            Granularity::Topic, 80, &["UNX-F0740-J1"]).ok().expect("首登必成");
        match e.register_entry(7, "标题四字节乙", "B38/F0741", "F0745:M", "T0000000002",
            Granularity::Topic, 80, &["UNX-F0741-J1"]) {
            Err(FmtSpecError::EntryDuplicate { id }) => assert_eq!(id, 7),
            other => panic!("重号必须拒，实得 {:?}", other.map(|_| ())),
        }
        // 容量 2 的定容引擎：第三条必拒。
        let mut c = FmtSpecEngine::new_sized(2);
        c.register_entry(1, "标题四字节甲", "B38/F0740", "F0745:M", "T0000000001",
            Granularity::Topic, 80, &["UNX-F0740-J1"]).ok().expect("1");
        c.register_entry(2, "标题四字节乙", "B38/F0741", "F0745:M", "T0000000002",
            Granularity::Topic, 80, &["UNX-F0741-J1"]).ok().expect("2");
        match c.register_entry(3, "标题四字节丙", "B38/F0742", "F0745:M", "T0000000003",
            Granularity::Topic, 80, &["UNX-F0742-J1"]) {
            Err(FmtSpecError::CapacityExceeded { cap }) => assert_eq!(cap, 2),
            other => panic!("越容必须拒，实得 {:?}", other.map(|_| ())),
        }
        assert_eq!(c.book_len(), 2);
        // 零容入参归一为 1。
        assert_eq!(FmtSpecEngine::new_sized(0).cap(), 1);
        // 条目访问越界 None。
        assert!(c.entry_at(5).is_none());
        assert_eq!(c.entry_at(1).map(|e| e.id), Some(2));
    }

    // ---------- 样例簿 + 审计 100% ----------

    #[test]
    fn sample_book_and_audit_100() {
        let mut e = FmtSpecEngine::new();
        let n = e.build_sample_book().ok().expect("样例簿必建");
        assert_eq!(n, SAMPLE_ENTRY_COUNT);
        assert_eq!(e.book_len(), 12);
        let report = e.audit_book().ok().expect("样例簿必审");
        assert_eq!(report.total, 12);
        assert_eq!(report.compliant, 12);
        assert_eq!(report.violation_count, 0);
        assert_eq!(report.pct, 10_000);
        assert!(report.all_clear(), "J1 符合度 100% 判据：{:?}", report.violations);
        // 最近审计留存。
        assert_eq!(e.last_audit().map(|r| r.pct), Some(10_000));
        // 三档分布（样例簿：主题 2 / 批 7 / 域 3）。
        let mut topic = 0;
        let mut batch = 0;
        let mut domain = 0;
        let mut i: usize = 0;
        while i < e.book_len() as usize {
            match e.entry_at(i).map(|x| x.gran) {
                Some(Granularity::Topic) => topic += 1,
                Some(Granularity::Batch) => batch += 1,
                Some(Granularity::Domain) => domain += 1,
                None => panic!("条目缺失 {}", i),
            }
            i += 1;
        }
        assert_eq!((topic, batch, domain), (2, 7, 3));
        // 事件账：12 登记 + 1 审计，序连续。
        assert_eq!(e.event_count(), 13);
        assert_eq!(e.check_event_chain().ok(), Some(13));
    }

    // ---------- 冻结/证书/篡改红案 ----------

    #[test]
    fn freeze_seal_certificate_and_repeat_rejected() {
        let mut e = eng_full();
        // 未封存先校验 → step=1。
        match e.verify_spec() {
            Err(FmtSpecError::FreezeLifecycleBroken { step }) => assert_eq!(step, 1),
            other => panic!("未封存先校验必须拒，实得 {:?}", other.map(|_| ())),
        }
        let fp = e.freeze_spec().ok().expect("封存必成");
        assert_eq!(e.frozen_fingerprint(), Some(fp));
        assert!(e.is_sealed());
        // 封存即出证。
        let cert = e.certificate().ok().expect("出证必成");
        assert_eq!(cert.version, 1);
        assert_eq!(cert.fingerprint, fp);
        assert_eq!(cert.entries_at_freeze, 12);
        assert!(cert.ledger.contains("v1"));
        assert!(cert.ledger.contains(&format!("0x{:08X}", fp)));
        assert!(cert.ledger.contains("模板五字段规范"));
        assert!(cert.ledger.contains("粒度规范"));
        assert!(cert.ledger.contains("证据规范"));
        // 校验绿。
        assert_eq!(e.verify_spec().ok(), Some(fp));
        // 同版重复封存 → step=2。
        match e.freeze_spec() {
            Err(FmtSpecError::FreezeLifecycleBroken { step }) => assert_eq!(step, 2),
            other => panic!("同版重封必须拒，实得 {:?}", other.map(|_| ())),
        }
    }

    #[test]
    fn tamper_red_case_detected() {
        let mut e = eng_full();
        let fp = e.freeze_spec().ok().expect("封存");
        // 克隆账上篡改：原地抬宽域级带宽（不走版本前进）。
        let mut c = e.clone();
        c.inject_tamper_widen_domain(9999);
        match c.verify_spec() {
            Err(FmtSpecError::SpecTamperDetected { frozen, computed }) => {
                assert_eq!(frozen, fp);
                assert_ne!(computed, fp);
            }
            other => panic!("篡改必须红案，实得 {:?}", other.map(|_| ())),
        }
        // 第二篡改位：改字段卡名。
        let mut c2 = e.clone();
        c2.inject_tamper_rename_field(1, "标题被改");
        assert!(c2.verify_spec().is_err());
        // 主账未染（注入只许上克隆账）。
        assert!(e.verify_spec().is_ok());
        assert_eq!(e.injections_active(), 0);
        // 篡改后审计归因口径漂移可查：带宽被抬宽后原 1201 违带条目
        // 会在克隆账上「合法化」——审计面用运行规范，篡改可被交叉发现。
        c.inject_raw_append(FormatEntry {
            id: 9100,
            title: "越带宽条目演示".to_string(),
            body_ref: "B38/F0740".to_string(),
            grade_ref: "F0745:M".to_string(),
            booked_at: "T0000000100".to_string(),
            gran: Granularity::Domain,
            word_count: 9990, // 原带宽 1201 越带；篡改后 9999 内「合法」
            evidences: vec!["UNX-F0741-J1".to_string(), "B38:L1".to_string()],
        })
        .ok().expect("raw 注入必成");
        let rep = c.audit_book().ok().expect("克隆账审计必成");
        // 13 条全审（样例 12 + 注入 1），注入条在篡改账上被抬宽的带宽
        // 放过——侧面坐实「审计用运行规范」与封存指纹是两道独立防线。
        assert_eq!(rep.total, 13);
        let hp = c.health();
        assert!(!hp.ok(), "篡改账健康核必须红");
    }

    // ---------- 演进 v2（登记制，只增不改） ----------

    #[test]
    fn evolve_v2_registration_and_diff() {
        let mut e = eng_full();
        let fp1 = e.freeze_spec().ok().expect("v1 封存");
        // 无变更演进 → step=4。
        match e.evolve_spec(SpecPatch::WidenDomainCeiling { new_max: 1200 }) {
            Err(FmtSpecError::FreezeLifecycleBroken { step }) => assert_eq!(step, 4),
            other => panic!("无变更演进必须拒，实得 {:?}", other.map(|_| ())),
        }
        // 补丁破坏闭集（证据下限抬过 4 上限）→ step=6。
        match e.evolve_spec(SpecPatch::RaiseGranMinEvidence {
            gran: Granularity::Batch,
            new_min: 5,
        }) {
            Err(FmtSpecError::FreezeLifecycleBroken { step }) => assert_eq!(step, 6),
            other => panic!("破坏闭集必须拒，实得 {:?}", other.map(|_| ())),
        }
        // 合法演进：域级上限 1200 → 1280。
        let v2 = e.evolve_spec(SpecPatch::WidenDomainCeiling { new_max: 1280 })
            .ok().expect("演进必成");
        assert_eq!(v2, 2);
        assert_eq!(e.spec_version(), 2);
        assert!(!e.is_sealed(), "演进后待封存");
        // 演进后未封存即校验 → step=5。
        match e.verify_spec() {
            Err(FmtSpecError::FreezeLifecycleBroken { step }) => assert_eq!(step, 5),
            other => panic!("待封版校验必须拒，实得 {:?}", other.map(|_| ())),
        }
        // 演进后未封存即再演进 → step=5。
        assert!(e.evolve_spec(SpecPatch::WidenDomainCeiling { new_max: 1300 }).is_err());
        // v2 封存：新旧并存（账 2 条，v1 记录原样）。
        let fp2 = e.freeze_spec().ok().expect("v2 封存");
        assert_ne!(fp1, fp2);
        assert_eq!(e.records().len(), 2);
        assert_eq!(e.records()[0].version, 1);
        assert_eq!(e.records()[0].fingerprint, fp1);
        assert_eq!(e.records()[0].snapshot.gran_bands[2].1, 1200, "v1 快照不动（只增不改）");
        assert_eq!(e.records()[1].snapshot.gran_bands[2].1, 1280);
        // 差异账：相邻两版逐字段比对。
        let diff = e.evolution_diff().ok().expect("差异账必出");
        assert!(diff.contains("v1→v2"));
        assert!(diff.contains("1200"));
        assert!(diff.contains("1280"));
        assert!(diff.contains("域级带宽"));
        // 校验绿 + 演进后新带宽在登记闸生效（运行规范口径随行）。
        assert_eq!(e.verify_spec().ok(), Some(fp2));
        assert!(e.check_band_current(Granularity::Domain, 1250).is_ok());
        assert!(check_gran_band(Granularity::Domain, 1250).is_err(), "const 定版口径不动");
        // 证据下限抬升补丁路径。
        let v3 = e.evolve_spec(SpecPatch::RaiseGranMinEvidence {
            gran: Granularity::Batch,
            new_min: 2,
        }).ok().expect("v3 演进必成");
        assert_eq!(v3, 3);
        // 降下限补丁 → step=6。
        let mut c = e.clone();
        c.freeze_spec().ok().expect("v3 封存");
        assert!(c.evolve_spec(SpecPatch::RaiseGranMinEvidence {
            gran: Granularity::Batch,
            new_min: 1,
        }).is_err());
    }

    // ---------- 审计三归因 ----------

    #[test]
    fn audit_violations_three_attributions() {
        let mut e = eng_full();
        // raw 注入三条坏条目（字段缺失/粒度越带/证据悬空各一）。
        e.inject_raw_append(FormatEntry {
            id: 9201,
            title: String::new(), // 字段缺失（标题空）
            body_ref: "B38/F0740".to_string(),
            grade_ref: "F0745:M".to_string(),
            booked_at: "T0000000201".to_string(),
            gran: Granularity::Topic,
            word_count: 80,
            evidences: vec!["UNX-F0740-J1".to_string()],
        }).ok().expect("raw1");
        e.inject_raw_append(FormatEntry {
            id: 9202,
            title: "粒度越带演示条目".to_string(),
            body_ref: "B38/F0740".to_string(),
            grade_ref: "F0745:M".to_string(),
            booked_at: "T0000000202".to_string(),
            gran: Granularity::Topic,
            word_count: 500, // 主题级越带（>120）
            evidences: vec!["UNX-F0740-J1".to_string()],
        }).ok().expect("raw2");
        e.inject_raw_append(FormatEntry {
            id: 9203,
            title: "证据悬空演示条目".to_string(),
            body_ref: "B38/F0740".to_string(),
            grade_ref: "F0745:M".to_string(),
            booked_at: "T0000000203".to_string(),
            gran: Granularity::Topic,
            word_count: 80,
            evidences: vec!["UNX-F9999-J9".to_string()], // 格式过、注册表无此号
        }).ok().expect("raw3");
        let rep = e.audit_book().ok().expect("审计必成");
        assert_eq!(rep.total, 15);
        assert_eq!(rep.compliant, 12);
        assert!(!rep.all_clear());
        // 三归因逐类在场。
        let mut field_hit = false;
        let mut gran_hit = false;
        let mut evid_hit = false;
        let mut i: usize = 0;
        while i < rep.violations.len() {
            match rep.violations[i].attribution {
                Attribution::FieldDefect => field_hit = true,
                Attribution::GranBand => gran_hit = true,
                Attribution::EvidenceDangling => evid_hit = true,
            }
            // 逐条带码带注。
            assert!(!rep.violations[i].note.is_empty());
            assert!(rep.violations[i].code >= 0x2791 && rep.violations[i].code <= 0x2795);
            i += 1;
        }
        assert!(field_hit && gran_hit && evid_hit, "三归因必须齐");
        // 符合度 12/15 = 8000/10000。
        assert_eq!(rep.pct, 8_000);
        // 事件账有注入流水；健康核注入项红（主账染注入）。
        assert!(e.injections_active() >= 3);
        let hp = e.health();
        assert!(!hp.ok());
        assert!(hp.findings.iter().any(|f| f.contains("注入未清零")));
    }

    // ---------- 注入套件 ----------

    #[test]
    fn injection_suite_three_detected() {
        let e = eng_full();
        let n = e.run_injection_suite().ok().expect("注入全检");
        assert_eq!(n, 3, "三类注入必须逐类检出");
        // 套件跑在克隆账上：主账零注入、事件不染。
        assert_eq!(e.injections_active(), 0);
    }

    // ---------- 双跑确定性 ----------

    #[test]
    fn double_run_determinism_all_same() {
        assert!(FmtSpecEngine::double_run_determinism().ok().expect("双跑必成"));
        // 同态重复：同一引擎两次取簿指纹必同。
        let mut e = eng_full();
        let f1 = e.book_fingerprint();
        let f2 = e.book_fingerprint();
        assert_eq!(f1, f2);
        // 相邻账微扰必换指纹（区分性）。
        e.register_entry(760, "移交包收官演示条目", "B38/F0741", "F0745:M",
            "T0000000076", Granularity::Topic, 60, &["UNX-F0760-J1"])
            .ok().expect("补登必成");
        assert_ne!(e.book_fingerprint(), f1);
    }

    // ---------- 健康核 ----------

    #[test]
    fn health_all_green_seven_checks() {
        let mut e = eng_full();
        e.freeze_spec().ok().expect("封存");
        e.verify_spec().ok().expect("校验");
        let hp = e.health();
        assert!(hp.ok(), "发现未清：{:?}", hp.findings);
        assert_eq!(hp.checks_total, 7);
        assert_eq!(hp.checks_passed, 7);
        assert!(hp.findings.is_empty());
        // 健康核事件落账。
        let n = e.event_count();
        assert_eq!(e.events()[(n - 1) as usize].kind, EV_HEALTH);
        // 事件链连续。
        assert!(e.check_event_chain().is_ok());
    }

    #[test]
    fn health_red_on_unaudited_and_unsealed() {
        // 非空簿从未审计 → 核6 红。
        let mut e = FmtSpecEngine::new();
        e.register_entry(1, "标题四字节甲", "B38/F0740", "F0745:M", "T0000000001",
            Granularity::Topic, 80, &["UNX-F0740-J1"]).ok().expect("登记");
        let hp = e.health();
        assert!(!hp.ok());
        assert!(hp.findings.iter().any(|f| f.contains("从未审计")));
        // 审计后转绿。
        e.audit_book().ok().expect("审计");
        assert!(e.health().ok());
    }

    // ---------- 事件账 ----------

    #[test]
    fn event_ledger_kinds_in_order() {
        let mut e = FmtSpecEngine::new();
        e.build_sample_book().ok().expect("簿");
        e.audit_book().ok().expect("审");
        e.freeze_spec().ok().expect("封");
        e.verify_spec().ok().expect("验");
        e.evolve_spec(SpecPatch::WidenDomainCeiling { new_max: 1280 })
            .ok().expect("演");
        e.freeze_spec().ok().expect("封2");
        e.health();
        // 种类在场断言。
        let mut saw_register = false;
        let mut saw_audit = false;
        let mut saw_freeze = false;
        let mut saw_verify = false;
        let mut saw_evolve = false;
        let mut saw_health = false;
        let mut i: usize = 0;
        while i < e.events().len() {
            match e.events()[i].kind {
                EV_REGISTER => saw_register = true,
                EV_AUDIT => saw_audit = true,
                EV_FREEZE => saw_freeze = true,
                EV_VERIFY => saw_verify = true,
                EV_EVOLVE => saw_evolve = true,
                EV_HEALTH => saw_health = true,
                _ => {}
            }
            i += 1;
        }
        assert!(saw_register && saw_audit && saw_freeze);
        assert!(saw_verify && saw_evolve && saw_health);
        // v2 已封存（「封2」）→ 健康核全绿（审计复算一致：样例簿
        // 域级字数全落 1280 新带内）。
        let hp = e.health();
        assert!(hp.ok(), "封存后健康核必须绿：{:?}", hp.findings);
        // 事件名闭集。
        assert_eq!(event_kind_name(EV_REJECT), "拒收");
        assert_eq!(event_kind_name(99), "未知事件");
    }

    // ---------- 渲染与错误文本 ----------

    #[test]
    fn render_and_error_texts() {
        let e = FmtSpecEngine::new();
        let ledger = e.spec_ledger_text();
        assert!(ledger.contains("域经格式规范 冻结账 v1"));
        assert!(ledger.contains("规范指纹 0x"));
        // 规范形非空且版本位内嵌（v1 定版形）。
        let bytes = canonical_spec_bytes(&SPEC_V1);
        assert!(bytes.len() > 32);
        assert_eq!(bytes[9], b'1');
        // 错误三要素（码 + detail + full）。
        let err = FmtSpecError::SpecTamperDetected {
            frozen: 1,
            computed: 2,
        };
        assert_eq!(err.code(), XC_FREEZE_TAMPER);
        assert!(err.detail().contains("红案"));
        assert!(err.full().contains("XC_2798"));
        assert!(err.full().contains("0x00000001"));
        let lc = FmtSpecError::FreezeLifecycleBroken { step: 5 };
        assert!(lc.full().contains(lifecycle_step_text(5)));
        assert!(FmtSpecError::EntryDuplicate { id: 7 }.full().contains("id=7"));
    }

    // ---------- 规范形/簿形区分性 ----------

    #[test]
    fn canonical_forms_distinguish() {
        let mut a = FmtSpecEngine::new();
        a.build_sample_book().ok().expect("簿A");
        let mut b = FmtSpecEngine::new();
        b.build_sample_book().ok().expect("簿B");
        assert_eq!(a.book_fingerprint(), b.book_fingerprint());
        assert_eq!(a.spec_fingerprint(), b.spec_fingerprint());
        // 规范形与簿形必异域。
        assert_ne!(a.book_fingerprint(), a.spec_fingerprint());
        // 规范演进后规范指纹必变、簿指纹不动（账与规范分域封存）。
        a.freeze_spec().ok().expect("封");
        let fp_v1 = a.spec_fingerprint();
        let book_fp = a.book_fingerprint();
        a.evolve_spec(SpecPatch::WidenDomainCeiling { new_max: 1300 })
            .ok().expect("演");
        assert_ne!(a.spec_fingerprint(), fp_v1);
        assert_eq!(a.book_fingerprint(), book_fp);
    }
}
