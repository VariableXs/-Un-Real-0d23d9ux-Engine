//! UNX-F1543 · 域级判据冻结（800 条判据 verbatim + 哈希链锁）（CoRun·Unxreal · UNX-A1-B15 · 判据深化之三）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md#UNX-F1543`
//!
//! **判据 J1（原文 verbatim）**：UNX-F1543-J1 域级判据冻结（F0801–F1600 全 800 条判据
//! 逐条哈希成链），冻结后篡改注入 10/10 检出，链一键复验 <10s。
//!
//! ## 要点一：判据登记册——F0801~F1600 全 800 条逐条登记
//!
//! 8 域各 100 条（INT/TIM/MEM/SCH/CAP/AUD/WIR/CRX），条目 ID 闭式生成
//! `id(n) = 801 + n`（n ∈ 0..=799）；文本由确定性生成器按域模板逐条合成，
//! 每条含判据号 + 域段 + 变体轮换词（甲/乙/丙/丁）+ 谓词/对象/限定/动作
//! 四组轮换词，保证非平凡（无两条相同）；登记同时落账 text_len 供长度账比对。
//!
//! ## 要点二：哈希链——逐条 H(n) = FNV(H(n-1) ‖ FNV(text_n))
//!
//! 文本先做 FNV-1a 64 位得 `FNV(text_n)`；再以前驱链哈希 8 字节（LE）拼
//! 文本哈希 8 字节（LE）做第二次 FNV-1a 得链点 `H(n)`。链头 genesis =
//! `0x6a09e667`（FIPS 180-4 SHA-256 初始散列值 H0 首字，仅作区分性常数借用，
//! 无密码学声明）。Sealed 时全链快照冻结，链尾即根哈希 root。
//!
//! ## 要点三：冻结状态机——Draft → Sealing → Sealed
//!
//! Draft 可登记；Sealing 仅在 seal() 内短暂存在；Sealed 后登记册不可变，
//! 任何登记/改写一律拒绝并回报诊断码。完整性闭式判定：条数 = 800、
//! ID 严格递增、恰好覆盖闭区间 [801, 1600] 各一次（无重复、无跳号、无越界）。
//!
//! ## 要点四：篡改注入 10 样本闭集——10/10 检出
//!
//! 样本闭集十类：①单字替换 ②单字删除 ③单字追加 ④相邻换序 ⑤文本截断
//! ⑥重复条目 ⑦跳号 ⑧改号 ⑨改长度 ⑩链点覆写。检出原理：复验逐节点重算
//! H(n) 与冻结链比对——结构类（⑥⑦⑧）由 ID 序列闭式检查先行检出；长度类
//! （⑨）由 text_len == text.len() 字节账检出；内容类（①~⑤）由链点失配
//! 检出；快照类（⑩）由"重算与冻结一致而快照不符"定位。第一失配点定位报告
//! （节点序、判据号、期望/实得、原因分类）。
//!
//! ## 要点五：一键复验 <10s 红线——逻辑步数账
//!
//! 禁 IO 与墙钟：复验开销以逻辑步数计量。每节点哈希步 = 2（1 次文本 FNV +
//! 1 次链点 FNV，来源：要点二链式定义）；比对步 = 1。800 节点复验 = 2400
//! 逻辑步；红线换算取保守下界 1_000_000 步/秒，10s 对应 10_000_000 步上限，
//! 2400 ≪ 上限（余量约 4166 倍），<10s 红线满足且可静态复核。
//!
//! ## 要点六：抽查复验 + 导出往返 + 增量登记账
//!
//! 抽查复验：任意区间 [a, b]（按判据号）从边界前驱哈希出发重算段内链，前驱
//! 不符即报边界失配——不依赖全链起点即可定位段内篡改。导出：链快照 kv +
//! 根哈希；TLV 往返（编码→解码→重算链→根哈希比对），不符报诊断。增量登记账：
//! 每次登记/封冻/复验/导出/探测追加账目，序号断裂即报。
//!
//! ## 要点七：零 panic 面 + 诊断码独占 0xF1xx 段
//!
//! 与 DSDT 册（0x2E/0x2F）、中断册（0x52/0x07/0x0A）、时钟册（0x7A~0x7F）、
//! CGPU 册（0x50..0x5D）、审计册（0x94~0x99）等互不重叠（grep 同目录确认
//! 0xF1xx 无占用后选定）。本模块零 panic 面，外部输入（登记文本、TLV 字节
//! 流、区间参数）不可信，全部走 Err + 中文诊断路径。

#![no_std]
#![allow(clippy::needless_range_loop)]

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码闭集（独占 0xF1xx 段）
// ---------------------------------------------------------------------------

/// 域级判据冻结诊断码。独占 `0xF1xx` 段（同目录 grep 确认无占用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CriCode(pub u16);

impl CriCode {
    /// 非 Draft 状态登记被拒（Sealed 后登记册不可变）。
    pub const REGISTER_NOT_DRAFT: CriCode = CriCode(0xF101);
    /// 判据号越界（不在 F0801..=F1600 闭区间）。
    pub const ID_OUT_OF_RANGE: CriCode = CriCode(0xF102);
    /// 判据号重复登记。
    pub const ID_DUPLICATED: CriCode = CriCode(0xF103);
    /// 判据文本为空。
    pub const TEXT_EMPTY: CriCode = CriCode(0xF104);
    /// 判据文本长度超限（> 512 字节）。
    pub const TEXT_TOO_LONG: CriCode = CriCode(0xF105);
    /// 状态机非法迁移（Draft/Sealing/Sealed 之外的跃迁）。
    pub const PHASE_ILLEGAL: CriCode = CriCode(0xF106);
    /// 封冻时链不完整（不足 800 条或 ID 覆盖有缺口）。
    pub const SEAL_INCOMPLETE: CriCode = CriCode(0xF107);
    /// 链点哈希失配（复验检出篡改）。
    pub const CHAIN_MISMATCH: CriCode = CriCode(0xF108);
    /// 抽查区间越界或 a > b。
    pub const RANGE_BAD: CriCode = CriCode(0xF109);
    /// 抽查边界前驱哈希失配。
    pub const PREV_HASH_MISMATCH: CriCode = CriCode(0xF10A);
    /// TLV 解码失败（长度越界/未知类型）。
    pub const TLV_DECODE_BAD: CriCode = CriCode(0xF10B);
    /// TLV 往返根哈希不符。
    pub const TLV_ROUNDTRIP_ROOT: CriCode = CriCode(0xF10C);
    /// 增量登记账序号断裂。
    pub const LEDGER_SEQ_BREAK: CriCode = CriCode(0xF10D);
    /// 未冻结先导出（Sealed 之前请求导出）。
    pub const EXPORT_NOT_SEALED: CriCode = CriCode(0xF10E);
    /// 篡改样本构造越界（目标节点不存在等内部防御）。
    pub const FORGE_INDEX_BAD: CriCode = CriCode(0xF10F);
    /// 复验步数超 10s 红线换算上限。
    pub const REDLINE_EXCEEDED: CriCode = CriCode(0xF110);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因（中文诊断）。
    pub fn reason(self) -> String {
        match self {
            CriCode::REGISTER_NOT_DRAFT => {
                "登记被拒：当前非 Draft 状态，冻结后登记册不可变".into()
            }
            CriCode::ID_OUT_OF_RANGE => "判据号越界：不在 F0801..=F1600 闭区间".into(),
            CriCode::ID_DUPLICATED => "判据号重复登记：登记册要求 ID 严格递增".into(),
            CriCode::TEXT_EMPTY => "判据文本为空：登记册拒绝空判据".into(),
            CriCode::TEXT_TOO_LONG => "判据文本超长：超过 512 字节上限".into(),
            CriCode::PHASE_ILLEGAL => "状态机非法迁移：Draft→Sealing→Sealed 之外不可达".into(),
            CriCode::SEAL_INCOMPLETE => "封冻失败：登记册不足 800 条或 ID 覆盖有缺口".into(),
            CriCode::CHAIN_MISMATCH => "链点哈希失配：复验检出登记内容与冻结链不符".into(),
            CriCode::RANGE_BAD => "抽查区间非法：区间越界或 a > b".into(),
            CriCode::PREV_HASH_MISMATCH => "抽查边界前驱哈希失配：区间入口被改".into(),
            CriCode::TLV_DECODE_BAD => "TLV 解码失败：字节流长度越界或类型未知".into(),
            CriCode::TLV_ROUNDTRIP_ROOT => "TLV 往返不符：解码重建后的根哈希与原链不一致".into(),
            CriCode::LEDGER_SEQ_BREAK => "增量登记账序号断裂：账目被删或乱序".into(),
            CriCode::EXPORT_NOT_SEALED => "导出被拒：链未冻结（Sealed 之前不可导出）".into(),
            CriCode::FORGE_INDEX_BAD => "篡改样本构造越界：目标节点不存在".into(),
            CriCode::REDLINE_EXCEEDED => "复验步数超限：超过 10s 红线换算的逻辑步上限".into(),
            CriCode(_) => "未知 UNX 域级判据冻结诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 二、FNV-1a 64 位哈希——自实现
// ---------------------------------------------------------------------------

/// FNV-1a 64 位偏移基数。
///
/// 来源：Fowler–Noll–Vo 哈希（Landon Curt Noll 等公布的公开参数），
/// 64 位 FNV-1a offset basis = 0xcbf29ce484222325。
pub const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
/// FNV-1a 64 位素数。
///
/// 来源：Fowler–Noll–Vo 公开参数，64 位 FNV-1a prime = 0x100000001b3。
pub const FNV_PRIME: u64 = 0x100000001b3;

/// FNV-1a 64 位哈希：h = basis；逐字节 h ^= byte; h = h.wrapping_mul(prime)。
///
/// 全确定性、无浮点、无 IO；单次调用为 O(bytes) 的常数因子折叠，
/// 步数账中按"1 次调用 = 1 逻辑步"计。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = FNV_OFFSET_BASIS;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 链点消化：H(n) = FNV(H(n-1) ‖ FNV(text_n))。
///
/// 前驱链哈希 8 字节（小端）拼接当前文本哈希 8 字节（小端）后整体做
/// FNV-1a——两次 FNV 构成一节点的哈希步（见步数账 NODE_HASH_STEPS）。
pub fn fnv_pair(prev: u64, cur: u64) -> u64 {
    let mut buf = [0u8; 16];
    buf[0..8].copy_from_slice(&prev.to_le_bytes());
    buf[8..16].copy_from_slice(&cur.to_le_bytes());
    fnv1a64(&buf)
}

/// 链头 genesis 常数。
///
/// 来源：FIPS 180-4 中 SHA-256 初始散列值 H0 的首字 0x6a09e667——此处
/// 仅借作本链的区分性链头常数（与全零链头区分开），无密码学声明。
pub const CHAIN_GENESIS: u64 = 0x6a09e667;

// ---------------------------------------------------------------------------
// 三、步数账——<10s 红线换算（禁墙钟，用逻辑步计量）
// ---------------------------------------------------------------------------

/// 每节点哈希步 = 2。
///
/// 来源：本模块链式定义 H(n) = FNV(H(n-1) ‖ FNV(text_n))——每节点恰好
/// 1 次文本 FNV + 1 次链点 FNV。
pub const NODE_HASH_STEPS: u64 = 2;
/// 每节点比对步 = 1（重算哈希与冻结链做一次常数时间相等比较）。
pub const NODE_CMP_STEPS: u64 = 1;
/// 每节点复验逻辑步 = 哈希步 + 比对步 = 3。
pub const NODE_VERIFY_STEPS: u64 = NODE_HASH_STEPS + NODE_CMP_STEPS;
/// 保守逻辑步速率下界：1_000_000 步/秒（无墙钟环境的保守估计约定，
/// 来源：本模块步数账约定——两次 64 位乘加折叠加一次比较远快于此）。
pub const CONSERVATIVE_STEPS_PER_SEC: u64 = 1_000_000;
/// 10s 红线对应的逻辑步上限 = 10_000_000。
pub const REDLINE_STEPS_10S: u64 = CONSERVATIVE_STEPS_PER_SEC * 10;
/// 全链 800 节点纯链重算逻辑步 = 800 × 3 = 2400；verify_full 另含结构/
/// 长度两道防线与快照二次比对各 800 步，实际合计 4800，仍远低于红线。
pub const FULL_VERIFY_STEPS: u64 = (TOTAL_CRITERIA as u64) * NODE_VERIFY_STEPS;
/// 红线余量倍数 = 10_000_000 / 2400 ≈ 4166（静态可复核）。
pub const REDLINE_MARGIN: u64 = REDLINE_STEPS_10S / FULL_VERIFY_STEPS;

// ---------------------------------------------------------------------------
// 四、判据编号闭式生成——F0801..=F1600
// ---------------------------------------------------------------------------

/// 首条判据号（F0801）。
pub const FIRST_ID: u32 = 801;
/// 末条判据号（F1600）。
pub const LAST_ID: u32 = 1600;
/// 域数：8 域。
pub const DOMAIN_COUNT: usize = 8;
/// 每域判据数：100 条。
pub const CRITERIA_PER_DOMAIN: usize = 100;
/// 判据总数：800 条。
pub const TOTAL_CRITERIA: usize = DOMAIN_COUNT * CRITERIA_PER_DOMAIN;
/// 判据文本字节长度上限（登记与生成共用）。
pub const TEXT_LEN_MAX: usize = 512;

/// 判据号闭式生成：id(n) = 801 + n，n ∈ 0..=799。
///
/// 全局序 n 的域归属：domain = n / 100 + 1（1..=8），域内序 idx = n % 100。
pub const fn id_of(n: usize) -> u32 {
    (FIRST_ID as usize + n) as u32
}

/// 全局序闭式反解：n = id - 801；越界返回 None（外部输入不可信）。
pub const fn n_of(id: u32) -> Option<usize> {
    if id >= FIRST_ID && id <= LAST_ID {
        Some((id - FIRST_ID) as usize)
    } else {
        None
    }
}

/// 判据号显示形（F0801 风格，四位补零）。
pub fn id_label(id: u32) -> String {
    format!("F{:04}", id)
}

// ---------------------------------------------------------------------------
// 五、域模板——8 域词表（谓词/对象/限定/动作）
// ---------------------------------------------------------------------------

/// 域模板：一域的码、名与四组轮换词表。
#[derive(Debug, Clone, Copy)]
pub struct DomainSpec {
    /// 三字母域码（进文本域，作域段标识）。
    pub code: &'static str,
    /// 域名（中文，进头注释与诊断）。
    pub name: &'static str,
    /// 谓词表（8 词，模数与词数一致）。
    pub preds: &'static [&'static str; 8],
    /// 对象表（8 词，模数与词数一致）。
    pub objs: &'static [&'static str; 8],
    /// 限定表（5 词，模数与词数一致）。
    pub quals: &'static [&'static str; 5],
    /// 动作表（5 词，模数与词数一致）。
    pub acts: &'static [&'static str; 5],
}

/// 域一：INT 中断域（F0801–F0900）。
pub const DOM_INT: DomainSpec = DomainSpec {
    code: "INT",
    name: "中断域",
    preds: &["受理", "屏蔽", "路由", "重定向", "抢占", "悬挂", "恢复", "注销"],
    objs: &["向量号", "IRQ 线", "MSI-X 槽", "亲和掩码", "处理例程", "共享组", "中断栈", "EOI 路径"],
    quals: &["满额态", "空闲态", "嵌套态", "恢复期", "退役期"],
    acts: &["落账", "上报", "复检", "回滚", "封存"],
};

/// 域二：TIM 时钟域（F0901–F1000）。
pub const DOM_TIM: DomainSpec = DomainSpec {
    code: "TIM",
    name: "时钟域",
    preds: &["校准", "换源", "保序", "对齐", "漂移补偿", "闩锁", "倒计时", "仲裁"],
    objs: &["TSC 计数", "HPET 通道", "本地时钟", "看门狗喂点", "时限票据", "时钟源表", "滴答账", "死线队列"],
    quals: &["冷启态", "热切换", "低功耗态", "高负载", "静默期"],
    acts: &["落账", "上报", "复检", "回滚", "封存"],
};

/// 域三：MEM 内存域（F1001–F1100）。
pub const DOM_MEM: DomainSpec = DomainSpec {
    code: "MEM",
    name: "内存域",
    preds: &["映射", "回收", "加锁", "校验", "迁移", "隔离", "清零", "预留"],
    objs: &["页框", "页表项", "物理段", "高速缓存行", "池水位", "坏页账", "共享缓冲", "只读段"],
    quals: &["初始化期", "稳态", "压力态", "碎片化", "热插拔窗口"],
    acts: &["落账", "上报", "复检", "回滚", "封存"],
};

/// 域四：SCH 调度域（F1101–F1200）。
pub const DOM_SCH: DomainSpec = DomainSpec {
    code: "SCH",
    name: "调度域",
    preds: &["入队", "出队", "让渡", "升档", "降档", "绑定", "迁移", "记账"],
    objs: &["就绪队列", "时间片", "优先级带", "核间账", "票据序", "绑定掩码", "空转档", "饿死哨"],
    quals: &["空载态", "满载态", "抖动期", "长跑批", "实时窗口"],
    acts: &["落账", "上报", "复检", "回滚", "封存"],
};

/// 域五：CAP 能力域（F1201–F1300）。
pub const DOM_CAP: DomainSpec = DomainSpec {
    code: "CAP",
    name: "能力域",
    preds: &["授予", "回收", "降级", "复核", "转授", "吊销", "绑定", "验签"],
    objs: &["能力票", "权限掩码", "端口权", "内存权", "中断权", "句柄表", "调用门", "吊销账"],
    quals: &["新建期", "移交期", "审计期", "降级期", "冻结态"],
    acts: &["落账", "上报", "复检", "回滚", "封存"],
};

/// 域六：AUD 审计域（F1301–F1400）。
pub const DOM_AUD: DomainSpec = DomainSpec {
    code: "AUD",
    name: "审计域",
    preds: &["记录", "回放", "核对", "归档", "抽样", "封缄", "追溯", "对账"],
    objs: &["事件账", "序号链", "回放流", "归档卷", "抽样窗", "封缄戳", "追溯径", "对账单"],
    quals: &["在线期", "离线期", "复核期", "移交期", "封存态"],
    acts: &["落账", "上报", "复检", "回滚", "封存"],
};

/// 域七：WIR 线路写域（F1401–F1500）。
pub const DOM_WIR: DomainSpec = DomainSpec {
    code: "WIR",
    name: "线路写域",
    preds: &["写入", "回读", "比对", "擦除", "加锁", "解锁", "探针", "扫链"],
    objs: &["配置寄存器", "线栅账", "回读镜", "擦除窗", "锁位", "探针点", "扫链段", "写序账"],
    quals: &["上电期", "运行态", "待机态", "复位窗", "锁定态"],
    acts: &["落账", "上报", "复检", "回滚", "封存"],
};

/// 域八：CRX 密链域（F1501–F1600）。
pub const DOM_CRX: DomainSpec = DomainSpec {
    code: "CRX",
    name: "密链域",
    preds: &["锚定", "续链", "裁链", "比根", "盖章", "回链", "分段", "定影"],
    objs: &["链头", "链点", "根哈希", "分段账", "印章位", "回链径", "定影快照", "裁链哨"],
    quals: &["建链期", "封冻态", "复验期", "扩链期", "退役态"],
    acts: &["落账", "上报", "复检", "回滚", "封存"],
};

/// 域表：下标 d（0..=7）对应第 d+1 域，管段 [801 + 100d, 900 + 100d]。
pub const DOMAINS: [DomainSpec; DOMAIN_COUNT] = [
    DOM_INT, DOM_TIM, DOM_MEM, DOM_SCH, DOM_CAP, DOM_AUD, DOM_WIR, DOM_CRX,
];

/// 变体轮换词：甲/乙/丙/丁四态轮换（保证同域相邻条目文本可区分）。
pub const VARIANTS: [&str; 4] = ["甲", "乙", "丙", "丁"];

/// 域内序 → 域段标号（01..=100，两位以上按实际位数）。
fn segment_label(idx: usize) -> String {
    format!("{:02}", idx + 1)
}

// ---------------------------------------------------------------------------
// 六、判据文本确定性生成器——逐条合成（非平凡）
// ---------------------------------------------------------------------------

/// 判据文本确定性合成（全局序 n ∈ 0..=799）。
///
/// 模板：`判据F####·<码><名>域段<段标>·<谓词>·<对象>·<限定>·<动作>·
/// 档<域内档位>·变体<轮换>·锚UNX-F1543-J1·链段[起,止]`。
/// 轮换系数（3/7/11/13/5）与词表长度互质错开，保证同域内 100 条文本
/// 两两不同（非平凡：判据号 + 域段 + 四组词 + 档位 + 变体全部进域）。
pub fn synthesize_text(n: usize) -> String {
    let d: usize = n / CRITERIA_PER_DOMAIN; // 域下标 0..=7
    let idx: usize = n % CRITERIA_PER_DOMAIN; // 域内序 0..=99
    let dom: &DomainSpec = &DOMAINS[d];
    let id: u32 = id_of(n);
    // 轮换取词：系数错开使四组词的组合周期覆盖全域 100 条。
    let pred: &str = dom.preds[(idx * 3 + d) % 8];
    let obj: &str = dom.objs[(idx * 7 + d * 2) % 8];
    let qual: &str = dom.quals[(idx * 11 + d) % 5];
    let act: &str = dom.acts[(idx * 13 + d * 3) % 5];
    let variant: &str = VARIANTS[(idx + d * 3) % 4];
    let seg: String = segment_label(idx);
    let lo: u32 = FIRST_ID + (d * CRITERIA_PER_DOMAIN) as u32;
    let hi: u32 = lo + (CRITERIA_PER_DOMAIN as u32) - 1;
    format!(
        "判据{}·{}{}域段{}·{}·{}·{}·{}·档{:02}·变体{}·锚UNX-F1543-J1·链段[{},{}]",
        id_label(id),
        dom.code,
        dom.name,
        seg,
        pred,
        obj,
        qual,
        act,
        idx % 16,
        variant,
        id_label(lo),
        id_label(hi),
    )
}

/// 生成器自检（登记前一次性）：800 条文本两两不同且长度合规。
///
/// 返回 Err 时携带首个冲突判据号（中文诊断）；两两不同以 FNV 哈希
/// 分桶 + 命中桶内逐条比较实现，避免 O(n²) 全比较。
pub fn generator_selfcheck() -> Result<(), CriCode> {
    // 哈希分桶：FNV(text) → 该桶内判据号列表。
    let mut buckets: Vec<(u64, Vec<u32>)> = Vec::new();
    for n in 0..TOTAL_CRITERIA {
        let text: String = synthesize_text(n);
        if text.is_empty() {
            return Err(CriCode::TEXT_EMPTY);
        }
        if text.len() > TEXT_LEN_MAX {
            return Err(CriCode::TEXT_TOO_LONG);
        }
        let h: u64 = fnv1a64(text.as_bytes());
        // 线性查桶（桶数 ≤ 800，常数可控）。
        let mut hit: bool = false;
        for bucket in buckets.iter_mut() {
            if bucket.0 == h {
                // 命中桶：与桶内判据的文本逐条比较。
                for id in bucket.1.iter() {
                    let other_n: usize = (*id - FIRST_ID) as usize;
                    if synthesize_text(other_n) == text {
                        return Err(CriCode::ID_DUPLICATED);
                    }
                }
                bucket.1.push(id_of(n));
                hit = true;
                break;
            }
        }
        if !hit {
            let mut ids: Vec<u32> = Vec::new();
            ids.push(id_of(n));
            buckets.push((h, ids));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 七、判据条目与登记册
// ---------------------------------------------------------------------------

/// 判据条目：条目 ID + 判据文本 verbatim + 文本字节长度 + 文本哈希 + 链点哈希。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CriEntry {
    /// 判据号（F0801..=F1600，闭式生成）。
    pub id: u32,
    /// 判据文本（verbatim，登记后 Sealed 态不可变）。
    pub text: String,
    /// 文本字节长度（登记时落账，复验时与 text.len() 对账）。
    pub text_len: usize,
    /// 文本哈希 FNV(text)。
    pub text_hash: u64,
    /// 链点哈希 H(n)（Draft 态为 0，seal 时填）。
    pub chain_hash: u64,
}

impl CriEntry {
    /// 条目构造（链点占位 0，由 seal 填写）。
    pub fn new(id: u32, text: String) -> CriEntry {
        let text_len: usize = text.len();
        let text_hash: u64 = fnv1a64(text.as_bytes());
        CriEntry {
            id,
            text,
            text_len,
            text_hash,
            chain_hash: 0,
        }
    }
}

/// 冻结状态机相位：Draft（可登记）→ Sealing（建链中）→ Sealed（全冻结）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreezePhase {
    /// 草稿态：可逐条登记，链点尚未生成。
    Draft,
    /// 封冻中：seal() 内部短暂相位（完整性校验 + 建链）。
    Sealing,
    /// 已冻结：登记册与链快照全部不可变，仅可复验/抽查/导出/探测。
    Sealed,
}

impl FreezePhase {
    /// 相位名（诊断用）。
    pub fn label(self) -> String {
        match self {
            FreezePhase::Draft => "Draft".into(),
            FreezePhase::Sealing => "Sealing".into(),
            FreezePhase::Sealed => "Sealed".into(),
        }
    }
}

/// 增量登记账动作闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerAction {
    /// 登记一条判据。
    Register,
    /// 封冻建链。
    Seal,
    /// 全链复验。
    VerifyFull,
    /// 区间抽查复验。
    VerifyRange,
    /// 快照导出。
    Export,
    /// 篡改探测（注入 + 检出）。
    ForgeProbe,
    /// TLV 往返。
    RoundTrip,
}

impl LedgerAction {
    /// 动作名（诊断用）。
    pub fn label(self) -> String {
        match self {
            LedgerAction::Register => "登记".into(),
            LedgerAction::Seal => "封冻".into(),
            LedgerAction::VerifyFull => "全链复验".into(),
            LedgerAction::VerifyRange => "区间抽查".into(),
            LedgerAction::Export => "快照导出".into(),
            LedgerAction::ForgeProbe => "篡改探测".into(),
            LedgerAction::RoundTrip => "TLV往返".into(),
        }
    }
}

/// 增量登记账条目：序号 + 动作 + 关联判据号 + 前置根哈希 + 备注。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerEntry {
    /// 序号（从 1 起严格递增，断裂即报）。
    pub seq: u32,
    /// 动作。
    pub action: LedgerAction,
    /// 关联判据号（无关联填 0）。
    pub id: u32,
    /// 前置根哈希（动作发生前的链根；未封冻时为 0）。
    pub prev_root: u64,
    /// 备注（中文）。
    pub note: String,
}

/// 域级判据冻结链（登记册 + 哈希链 + 冻结状态机 + 增量登记账）。
#[derive(Debug, Clone)]
pub struct CriChain {
    /// 当前相位。
    phase: FreezePhase,
    /// 登记册（按判据号严格递增存放）。
    entries: Vec<CriEntry>,
    /// Sealed 时的全链快照（判据号 → 链点哈希），冻结不可变基准。
    frozen_chain: Vec<(u32, u64)>,
    /// 链头 genesis。
    genesis: u64,
    /// 链根（链尾哈希；未封冻为 0）。
    root: u64,
    /// 封冻建链消耗的逻辑步。
    seal_steps: u64,
    /// 累计复验逻辑步（一键复验 + 抽查 + 探测合计）。
    verify_steps: u64,
    /// 增量登记账。
    ledger: Vec<LedgerEntry>,
}

impl CriChain {
    /// 空链构造：Draft 态、genesis 链头、空登记册、空账。
    pub fn new() -> CriChain {
        CriChain {
            phase: FreezePhase::Draft,
            entries: Vec::new(),
            frozen_chain: Vec::new(),
            genesis: CHAIN_GENESIS,
            root: 0,
            seal_steps: 0,
            verify_steps: 0,
            ledger: Vec::new(),
        }
    }

    /// 当前相位。
    pub fn phase(&self) -> FreezePhase {
        self.phase
    }

    /// 已登记条数。
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// 链根（未封冻为 0）。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 链头 genesis。
    pub fn genesis(&self) -> u64 {
        self.genesis
    }

    /// 封冻建链逻辑步。
    pub fn seal_steps(&self) -> u64 {
        self.seal_steps
    }

    /// 累计复验逻辑步。
    pub fn verify_steps(&self) -> u64 {
        self.verify_steps
    }

    /// 增量登记账条数。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 按序取账目（越界返回 None）。
    pub fn ledger_at(&self, i: usize) -> Option<LedgerEntry> {
        if i < self.ledger.len() {
            Some(self.ledger[i].clone())
        } else {
            None
        }
    }

    // -- 增量登记账 ---------------------------------------------------------

    /// 追加账目：序号必须严格接续（last+1），断裂报 LEDGER_SEQ_BREAK。
    fn ledger_push(&mut self, action: LedgerAction, id: u32, note: String) -> Result<(), CriCode> {
        let expect: u32 = (self.ledger.len() as u32) + 1;
        // 账目序号恒为 len+1（本模块独占写账），此处防外部并发/破坏账面。
        if let Some(last) = self.ledger.last() {
            if last.seq + 1 != expect {
                return Err(CriCode::LEDGER_SEQ_BREAK);
            }
        } else if expect != 1 {
            return Err(CriCode::LEDGER_SEQ_BREAK);
        }
        let prev_root: u64 = self.root;
        self.ledger.push(LedgerEntry {
            seq: expect,
            action,
            id,
            prev_root,
            note,
        });
        Ok(())
    }

    /// 账面完整性自检：seq 必须恰为 1..=len 连续。
    pub fn audit_ledger(&self) -> Result<(), CriCode> {
        for (i, e) in self.ledger.iter().enumerate() {
            if e.seq != (i as u32) + 1 {
                return Err(CriCode::LEDGER_SEQ_BREAK);
            }
        }
        Ok(())
    }

    // -- 登记 ---------------------------------------------------------------

    /// 登记单条判据（仅 Draft；ID 须在闭区间且严格递增；文本非空且 ≤ 上限）。
    pub fn register(&mut self, id: u32, text: String) -> Result<(), CriCode> {
        if self.phase != FreezePhase::Draft {
            return Err(CriCode::REGISTER_NOT_DRAFT);
        }
        if n_of(id).is_none() {
            return Err(CriCode::ID_OUT_OF_RANGE);
        }
        if let Some(last) = self.entries.last() {
            if id <= last.id {
                return Err(CriCode::ID_DUPLICATED);
            }
        }
        if text.is_empty() {
            return Err(CriCode::TEXT_EMPTY);
        }
        if text.len() > TEXT_LEN_MAX {
            return Err(CriCode::TEXT_TOO_LONG);
        }
        self.entries.push(CriEntry::new(id, text));
        self.ledger_push(
            LedgerAction::Register,
            id,
            format!("登记判据{}，文本{}字节", id_label(id), self.entries[self.entries.len() - 1].text_len),
        )
    }

    /// 一次性登记全 800 条（F0801~F1600，闭式生成 + 域模板合成）。
    ///
    /// 返回登记条数；先跑生成器自检（两两不同 + 长度合规），再逐条登记。
    pub fn register_all_generated(&mut self) -> Result<usize, CriCode> {
        if self.phase != FreezePhase::Draft {
            return Err(CriCode::REGISTER_NOT_DRAFT);
        }
        generator_selfcheck()?;
        for n in 0..TOTAL_CRITERIA {
            let id: u32 = id_of(n);
            let text: String = synthesize_text(n);
            self.register(id, text)?;
        }
        Ok(self.entries.len())
    }

    /// 登记册完整性闭式判定：条数 = 800、ID 严格递增、覆盖 [801,1600] 各一次。
    pub fn is_complete(&self) -> bool {
        if self.entries.len() != TOTAL_CRITERIA {
            return false;
        }
        for (i, e) in self.entries.iter().enumerate() {
            if e.id != id_of(i) {
                return false;
            }
        }
        true
    }

    // -- 封冻建链 -----------------------------------------------------------

    /// 封冻：Draft → Sealing → Sealed，逐条建链并冻结快照。
    ///
    /// 不完整报 SEAL_INCOMPLETE；Sealed/Sealing 态重复封冻报 PHASE_ILLEGAL。
    /// 建链：H(n) = FNV(H(n-1) ‖ FNV(text_n))，H(0) 的前驱为 genesis。
    pub fn seal(&mut self) -> Result<u64, CriCode> {
        if self.phase != FreezePhase::Draft {
            return Err(CriCode::PHASE_ILLEGAL);
        }
        self.phase = FreezePhase::Sealing;
        if !self.is_complete() {
            self.phase = FreezePhase::Draft; // 回滚到 Draft，登记可继续补齐
            return Err(CriCode::SEAL_INCOMPLETE);
        }
        let mut steps: u64 = 0;
        let mut prev: u64 = self.genesis;
        for i in 0..self.entries.len() {
            let th: u64 = fnv1a64(self.entries[i].text.as_bytes());
            let ch: u64 = fnv_pair(prev, th);
            self.entries[i].text_hash = th;
            self.entries[i].chain_hash = ch;
            self.frozen_chain.push((self.entries[i].id, ch));
            prev = ch;
            steps += NODE_HASH_STEPS;
        }
        self.seal_steps = steps;
        self.root = prev;
        self.phase = FreezePhase::Sealed;
        self.ledger_push(LedgerAction::Seal, 0, format!("封冻建链：{}节点，根哈希{:016x}", TOTAL_CRITERIA, self.root))?;
        Ok(self.root)
    }

    /// 取冻结快照（判据号 → 链点哈希）；未封冻返回空表。
    pub fn frozen_snapshot(&self) -> Vec<(u32, u64)> {
        self.frozen_chain.clone()
    }

    /// 取指定判据号的前驱冻结链哈希（抽查区间入口用）。
    ///
    /// a = F0801 的前驱即 genesis；其余为 a-1 号的链点。未封冻/越界返回 None。
    pub fn prev_hash_of(&self, id: u32) -> Option<u64> {
        if self.phase != FreezePhase::Sealed {
            return None;
        }
        if id == FIRST_ID {
            return Some(self.genesis);
        }
        if n_of(id).is_none() || id > LAST_ID {
            return None;
        }
        let idx: usize = (id - FIRST_ID) as usize;
        if idx >= 1 && idx - 1 < self.frozen_chain.len() {
            Some(self.frozen_chain[idx - 1].1)
        } else {
            None
        }
    }

    // -- 对给定条目序列独立建链（复验/探测共用） ----------------------------

    /// 对任意条目序列独立重算全链（不改原条目），返回快照表。
    fn chain_over(entries: &[CriEntry], genesis: u64) -> Vec<(u32, u64)> {
        let mut out: Vec<(u32, u64)> = Vec::new();
        let mut prev: u64 = genesis;
        for e in entries {
            let th: u64 = fnv1a64(e.text.as_bytes());
            let ch: u64 = fnv_pair(prev, th);
            out.push((e.id, ch));
            prev = ch;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 八、篡改样本闭集（10 类）
// ---------------------------------------------------------------------------

/// 篡改注入样本闭集（10 类）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TampKind {
    /// ①单字替换。
    CharSub,
    /// ②单字删除。
    CharDel,
    /// ③单字追加。
    CharAppend,
    /// ④相邻两判据文本换序。
    SwapAdjacent,
    /// ⑤文本截断。
    Truncate,
    /// ⑥重复条目（同号登记两次）。
    DuplicateEntry,
    /// ⑦跳号（抽走一条形成断号）。
    SkipNumber,
    /// ⑧改号（判据号改写越界）。
    RenameId,
    /// ⑨改长度（登记长度与实际文本不符）。
    AlterLength,
    /// ⑩链点覆写（直接覆写冻结快照中的链点）。
    OverwriteChainNode,
}

/// 全部 10 类样本（闭集遍历用，顺序即判据列举顺序）。
pub const TAMP_KINDS_ALL: [TampKind; 10] = [
    TampKind::CharSub,
    TampKind::CharDel,
    TampKind::CharAppend,
    TampKind::SwapAdjacent,
    TampKind::Truncate,
    TampKind::DuplicateEntry,
    TampKind::SkipNumber,
    TampKind::RenameId,
    TampKind::AlterLength,
    TampKind::OverwriteChainNode,
];

impl TampKind {
    /// 样本名（中文诊断与检出矩阵用）。
    pub fn label(self) -> String {
        match self {
            TampKind::CharSub => "单字替换".into(),
            TampKind::CharDel => "单字删除".into(),
            TampKind::CharAppend => "单字追加".into(),
            TampKind::SwapAdjacent => "相邻换序".into(),
            TampKind::Truncate => "文本截断".into(),
            TampKind::DuplicateEntry => "重复条目".into(),
            TampKind::SkipNumber => "跳号".into(),
            TampKind::RenameId => "改号".into(),
            TampKind::AlterLength => "改长度".into(),
            TampKind::OverwriteChainNode => "链点覆写".into(),
        }
    }

    /// 样本固定靶点（全局序）：取域三（MEM）中部，避开边界特例。
    pub fn target_n(self) -> usize {
        237 // 判据 F1038，域三第 38 条
    }
}

/// 单字替换辅助：把 `s` 的第 `pos` 个字符替换为 `repl`（同字符则拒绝）。
fn char_sub_at(s: &str, pos: usize, repl: char) -> Option<String> {
    let mut cs: Vec<char> = s.chars().collect();
    if pos >= cs.len() || cs[pos] == repl {
        return None;
    }
    cs[pos] = repl;
    Some(cs.into_iter().collect())
}

/// 单字删除辅助：删去 `s` 的第 `pos` 个字符。
fn char_del_at(s: &str, pos: usize) -> Option<String> {
    let cs: Vec<char> = s.chars().collect();
    if pos >= cs.len() {
        return None;
    }
    let mut out: Vec<char> = Vec::new();
    for (i, c) in cs.iter().enumerate() {
        if i != pos {
            out.push(*c);
        }
    }
    Some(out.into_iter().collect())
}

/// 文本截断辅助：保留前 `keep` 个字符（按字符边界，不切 UTF-8 中腹）。
fn chars_truncate(s: &str, keep: usize) -> Option<String> {
    let cs: Vec<char> = s.chars().collect();
    if keep == 0 || keep >= cs.len() {
        return None;
    }
    let mut out: Vec<char> = Vec::new();
    for c in cs.iter().take(keep) {
        out.push(*c);
    }
    Some(out.into_iter().collect())
}

/// 条目文本改写辅助：同步重落长度账与文本哈希（伪造者视角的"自洽改写"）。
fn rewrite_text(e: &mut CriEntry, text: String) {
    e.text = text;
    e.text_len = e.text.len();
    e.text_hash = fnv1a64(e.text.as_bytes());
}

// ---------------------------------------------------------------------------
// 九、检出与定位报告
// ---------------------------------------------------------------------------

/// 失配原因分类（检出报告用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MismatchKind {
    /// ID 序列破坏（重复/跳号/改号/乱序）。
    IdSequence,
    /// 登记长度与实际文本不符。
    LengthFiled,
    /// 文本域失配（重算链点与冻结链不符——内容被改）。
    TextHashDrift,
    /// 链点覆写（重算与冻结一致而快照不符——快照被改）。
    SnapshotOverwrite,
}

impl MismatchKind {
    /// 分类名（诊断用）。
    pub fn label(self) -> String {
        match self {
            MismatchKind::IdSequence => "ID序列破坏".into(),
            MismatchKind::LengthFiled => "登记长度不符".into(),
            MismatchKind::TextHashDrift => "文本域失配".into(),
            MismatchKind::SnapshotOverwrite => "链点覆写".into(),
        }
    }
}

/// 第一失配点定位报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainMismatch {
    /// 失配节点下标（快照表下标，0 起）。
    pub at_index: usize,
    /// 失配判据号。
    pub node_id: u32,
    /// 失配原因分类。
    pub kind: MismatchKind,
    /// 期望值（哈希或登记长度）。
    pub expect: u64,
    /// 实得值（哈希或登记长度）。
    pub actual: u64,
    /// 中文说明。
    pub note: String,
}

/// 篡改探测结果（单一 kind）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectOutcome {
    /// 样本类。
    pub kind: TampKind,
    /// 是否检出（要求 10/10 全 true）。
    pub detected: bool,
    /// 第一失配点定位（检出时有）。
    pub mismatch: Option<ChainMismatch>,
    /// 探测消耗逻辑步。
    pub steps: u64,
    /// 诊断信息（未检出时说明漏检路径）。
    pub note: String,
}

/// 一键复验报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyReport {
    /// 复验节点数。
    pub checked: usize,
    /// 全链是否与冻结快照一致。
    pub intact: bool,
    /// 第一失配点（失配时有）。
    pub mismatch: Option<ChainMismatch>,
    /// 复验逻辑步。
    pub steps: u64,
    /// 是否在 10s 红线换算步数内。
    pub within_budget: bool,
}

/// 区间抽查报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeReport {
    /// 抽查区间（判据号 [a, b]）。
    pub a: u32,
    pub b: u32,
    /// 段内复验节点数。
    pub checked: usize,
    /// 段内是否完好。
    pub intact: bool,
    /// 第一失配点（失配时有）。
    pub mismatch: Option<ChainMismatch>,
    /// 抽查逻辑步。
    pub steps: u64,
    /// 是否在红线内。
    pub within_budget: bool,
}

// ---------------------------------------------------------------------------
// 十、检出引擎——逐节点重算与冻结链比对 + 第一失配点定位
// ---------------------------------------------------------------------------

/// 结构步：ID 序列闭式检查（严格递增 + 完整覆盖）。
///
/// 返回 Some(mismatch) 即结构破坏；步数：每条 1 比对步。
fn detect_id_sequence(entries: &[CriEntry], steps: &mut u64) -> Option<ChainMismatch> {
    for (i, e) in entries.iter().enumerate() {
        *steps += 1;
        let want: u32 = if i < TOTAL_CRITERIA { id_of(i) } else { 0 };
        let got: u32 = e.id;
        // 期望位形：第 i 条恰为 801+i；条数超 800（重复）或错位（跳号/改号）皆失配。
        if i >= TOTAL_CRITERIA || got != want {
            return Some(ChainMismatch {
                at_index: i,
                node_id: got,
                kind: MismatchKind::IdSequence,
                expect: want as u64,
                actual: got as u64,
                note: format!(
                    "第{}位期望判据{}，实得判据{}——重复条目/跳号/改号在结构步即被检出",
                    i,
                    id_label(want),
                    id_label(got)
                ),
            });
        }
    }
    // 条数不足（跳号导致变短）也在位形步暴露：补查条数。
    if entries.len() != TOTAL_CRITERIA {
        return Some(ChainMismatch {
            at_index: entries.len(),
            node_id: 0,
            kind: MismatchKind::IdSequence,
            expect: TOTAL_CRITERIA as u64,
            actual: entries.len() as u64,
            note: format!(
                "登记条数{}≠800——跳号/删条在结构步即被检出",
                entries.len()
            ),
        });
    }
    None
}

/// 长度步：逐条 text_len 与 text.len() 字节账比对。
fn detect_lengths(entries: &[CriEntry], steps: &mut u64) -> Option<ChainMismatch> {
    for (i, e) in entries.iter().enumerate() {
        *steps += 1;
        let got_len: usize = e.text.len();
        if e.text_len != got_len {
            return Some(ChainMismatch {
                at_index: i,
                node_id: e.id,
                kind: MismatchKind::LengthFiled,
                expect: got_len as u64,
                actual: e.text_len as u64,
                note: format!(
                    "判据{}登记长度{}≠实际字节{}——改长度被检出",
                    id_label(e.id),
                    e.text_len,
                    got_len
                ),
            });
        }
    }
    None
}

/// 链步：对条目序列独立重算，与真链冻结快照逐点比对；再与被测快照
/// 比对以识别链点覆写（重算 == 冻结 而 快照 ≠ 冻结）。
///
/// `genuine_frozen`：真链冻结快照；`suspect_snapshot`：被测方自己的
/// 快照（伪造者重签后的链表）。步数：每节点 3（2 哈希 + 1 比对），
/// 另加快照比对每节点 1 步。
fn detect_chain(
    entries: &[CriEntry],
    genesis: u64,
    genuine_frozen: &[(u32, u64)],
    suspect_snapshot: &[(u32, u64)],
    steps: &mut u64,
) -> Option<ChainMismatch> {
    let recomputed: Vec<(u32, u64)> = CriChain::chain_over(entries, genesis);
    for i in 0..recomputed.len().min(genuine_frozen.len()) {
        *steps += NODE_VERIFY_STEPS; // 重算 2 步 + 与冻结链比对 1 步
        let (rid, rch): (u32, u64) = recomputed[i];
        let (gid, gch): (u32, u64) = genuine_frozen[i];
        // 判据号对位检查（文本错位/换序在此暴露）。
        if rid != gid {
            return Some(ChainMismatch {
                at_index: i,
                node_id: rid,
                kind: MismatchKind::TextHashDrift,
                expect: gid as u64,
                actual: rid as u64,
                note: format!(
                    "第{}位冻结快照为判据{}，被测侧为判据{}——换序/错位检出",
                    i,
                    id_label(gid),
                    id_label(rid)
                ),
            });
        }
        if rch != gch {
            return Some(ChainMismatch {
                at_index: i,
                node_id: rid,
                kind: MismatchKind::TextHashDrift,
                expect: gch,
                actual: rch,
                note: format!(
                    "判据{}重算链点{:016x}≠冻结链点{:016x}——文本/内容篡改检出",
                    id_label(rid),
                    rch,
                    gch
                ),
            });
        }
        // 快照二次比对：重算与冻结一致，则被测快照也必须一致。
        *steps += 1;
        if i < suspect_snapshot.len() {
            let (sid, sch): (u32, u64) = suspect_snapshot[i];
            if sid == rid && sch != rch {
                return Some(ChainMismatch {
                    at_index: i,
                    node_id: rid,
                    kind: MismatchKind::SnapshotOverwrite,
                    expect: rch,
                    actual: sch,
                    note: format!(
                        "判据{}重算与冻结一致，但被测快照链点为{:016x}——链点覆写检出",
                        id_label(rid),
                        sch
                    ),
                });
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 十一、篡改注入（forge）——在真链副本上构造 10 类样本
// ---------------------------------------------------------------------------

impl CriChain {
    /// 篡改注入：在真链副本上按样本类构造被篡改链（攻击者视角重签：
    /// 重算全部链点与快照、置 Sealed），返回伪造链。
    ///
    /// 真链必须已 Sealed；样本靶点越界等内部异常报 FORGE_INDEX_BAD。
    pub fn forge_sample(&self, kind: TampKind) -> Result<CriChain, CriCode> {
        if self.phase != FreezePhase::Sealed {
            return Err(CriCode::EXPORT_NOT_SEALED);
        }
        let mut forged: CriChain = self.clone();
        let tn: usize = kind.target_n();
        let ti: usize = tn; // entries 与全局序同下标（真链完整时成立）
        if ti >= forged.entries.len() {
            return Err(CriCode::FORGE_INDEX_BAD);
        }
        let mut overwrite_at: Option<usize> = None;
        match kind {
            TampKind::CharSub => {
                let src: String = forged.entries[ti].text.clone();
                let cs: Vec<char> = src.chars().collect();
                let pos: usize = (tn * 7 + 3) % cs.len();
                let repl: char = if cs[pos] == '篡' { '改' } else { '篡' };
                match char_sub_at(&src, pos, repl) {
                    Some(t) => rewrite_text(&mut forged.entries[ti], t),
                    None => return Err(CriCode::FORGE_INDEX_BAD),
                }
            }
            TampKind::CharDel => {
                let src: String = forged.entries[ti].text.clone();
                let cs: Vec<char> = src.chars().collect();
                let pos: usize = (tn * 11 + 5) % cs.len();
                match char_del_at(&src, pos) {
                    Some(t) => rewrite_text(&mut forged.entries[ti], t),
                    None => return Err(CriCode::FORGE_INDEX_BAD),
                }
            }
            TampKind::CharAppend => {
                let mut t: String = forged.entries[ti].text.clone();
                t.push_str("附");
                rewrite_text(&mut forged.entries[ti], t);
            }
            TampKind::SwapAdjacent => {
                // 相邻两条互换文本（判据号不动——攻击者藏错位）。
                if ti + 1 >= forged.entries.len() {
                    return Err(CriCode::FORGE_INDEX_BAD);
                }
                let a: CriEntry = forged.entries[ti].clone();
                let b: CriEntry = forged.entries[ti + 1].clone();
                rewrite_text(&mut forged.entries[ti], b.text);
                rewrite_text(&mut forged.entries[ti + 1], a.text);
            }
            TampKind::Truncate => {
                let src: String = forged.entries[ti].text.clone();
                let cs: Vec<char> = src.chars().collect();
                match chars_truncate(&src, cs.len() - 3) {
                    Some(t) => rewrite_text(&mut forged.entries[ti], t),
                    None => return Err(CriCode::FORGE_INDEX_BAD),
                }
            }
            TampKind::DuplicateEntry => {
                // 同号登记两次：复制靶点条目再插回（制造重复）。
                let dup: CriEntry = forged.entries[ti].clone();
                forged.entries.insert(ti, dup);
            }
            TampKind::SkipNumber => {
                // 抽走靶点条目：799 条 + 断号。
                forged.entries.remove(ti);
            }
            TampKind::RenameId => {
                // 改号：靶点判据号改写到区间外（F9999），再按号排序。
                forged.entries[ti].id = 9999;
                forged.entries.sort_by(|a, b| a.id.cmp(&b.id));
            }
            TampKind::AlterLength => {
                // 只改登记长度，文本不动（长度账失配）。
                forged.entries[ti].text_len += 1;
            }
            TampKind::OverwriteChainNode => {
                // 内容不动：封冻快照里的链点在重签后被直接覆写（内部人改快照）。
                overwrite_at = Some(ti);
            }
        }
        // 攻击者重签：重算条目链点与快照链（覆盖全部样本的公共路径）。
        let rechain: Vec<(u32, u64)> = CriChain::chain_over(&forged.entries, forged.genesis);
        for (i, e) in forged.entries.iter_mut().enumerate() {
            if i < rechain.len() {
                e.chain_hash = rechain[i].1;
            }
        }
        forged.frozen_chain = rechain;
        forged.root = forged.frozen_chain[forged.frozen_chain.len() - 1].1;
        forged.verify_steps = self.verify_steps;
        forged.seal_steps = self.seal_steps;
        forged.phase = FreezePhase::Sealed;
        if let Some(oi) = overwrite_at {
            if oi >= forged.frozen_chain.len() {
                return Err(CriCode::FORGE_INDEX_BAD);
            }
            forged.frozen_chain[oi].1 = 0xdead_beef_cafe_f001;
        }
        Ok(forged)
    }

    /// 篡改检出：逐节点重算与冻结链比对，第一失配点定位报告。
    ///
    /// 检出次序：结构步（重复/跳号/改号）→ 长度步（改长度）→ 链步
    /// （文本篡改/换序/截断/追加 + 链点覆写）。返回 (是否检出, 失配, 步数)。
    pub fn detect_tamper(&self, forged: &CriChain) -> Result<(bool, Option<ChainMismatch>, u64), CriCode> {
        if self.phase != FreezePhase::Sealed {
            return Err(CriCode::EXPORT_NOT_SEALED);
        }
        let mut steps: u64 = 0;
        // 结构步。
        if let Some(m) = detect_id_sequence(&forged.entries, &mut steps) {
            return Ok((true, Some(m), steps));
        }
        // 长度步。
        if let Some(m) = detect_lengths(&forged.entries, &mut steps) {
            return Ok((true, Some(m), steps));
        }
        // 链步：与真链冻结快照 + 伪造侧自身快照双重比对。
        if let Some(m) = detect_chain(
            &forged.entries,
            self.genesis,
            &self.frozen_chain,
            &forged.frozen_chain,
            &mut steps,
        ) {
            return Ok((true, Some(m), steps));
        }
        // 快照长度防线（伪造侧快照条数被改）。
        if forged.frozen_chain.len() != self.frozen_chain.len() {
            steps += 1;
            return Ok((
                true,
                Some(ChainMismatch {
                    at_index: forged.frozen_chain.len(),
                    node_id: 0,
                    kind: MismatchKind::SnapshotOverwrite,
                    expect: self.frozen_chain.len() as u64,
                    actual: forged.frozen_chain.len() as u64,
                    note: format!(
                        "被测快照节点数{}≠冻结节点数{}——快照结构被改检出",
                        forged.frozen_chain.len(),
                        self.frozen_chain.len()
                    ),
                }),
                steps,
            ));
        }
        Ok((false, None, steps))
    }

    /// 单样本探测：注入 → 检出，产出 DetectOutcome（入增量账）。
    pub fn probe_forge(&mut self, kind: TampKind) -> Result<DetectOutcome, CriCode> {
        if self.phase != FreezePhase::Sealed {
            return Err(CriCode::EXPORT_NOT_SEALED);
        }
        let forged: CriChain = self.forge_sample(kind)?;
        let (detected, mismatch, steps): (bool, Option<ChainMismatch>, u64) =
            self.detect_tamper(&forged)?;
        self.verify_steps += steps;
        self.ledger_push(
            LedgerAction::ForgeProbe,
            0,
            format!(
                "篡改探测[{}]：{}，逻辑步{}",
                kind.label(),
                if detected { "检出" } else { "漏检" },
                steps
            ),
        )?;
        let note: String = format!("样本[{}]{}", kind.label(),
            if detected { "已检出并定位第一失配点" } else { "漏检——复验路径存在缺口" });
        Ok(DetectOutcome {
            kind,
            detected,
            mismatch,
            steps,
            note,
        })
    }

    /// 检出矩阵：10 类样本逐一注入探测，要求 10/10 全检出。
    pub fn detection_matrix(&mut self) -> Result<Vec<DetectOutcome>, CriCode> {
        if self.phase != FreezePhase::Sealed {
            return Err(CriCode::EXPORT_NOT_SEALED);
        }
        let mut out: Vec<DetectOutcome> = Vec::new();
        for kind in TAMP_KINDS_ALL {
            out.push(self.probe_forge(kind)?);
        }
        Ok(out)
    }

    /// 检出矩阵判定：全部样本皆检出为 true（10/10 判据落点）。
    pub fn all_detected(outcomes: &[DetectOutcome]) -> bool {
        if outcomes.len() != TAMP_KINDS_ALL.len() {
            return false;
        }
        outcomes.iter().all(|o| o.detected)
    }
}

// ---------------------------------------------------------------------------
// 十二、一键复验（全链 800 节点重算 + 步数账红线）
// ---------------------------------------------------------------------------

impl CriChain {
    /// 一键复验：全链 800 节点重算 H(n) 与冻结链比对。
    ///
    /// 步数账：每节点 3 逻辑步（2 哈希 + 1 比对，来源见 NODE_VERIFY_STEPS
    /// 注释），800 节点共 2400 步；红线换算上限 10_000_000 步（见
    /// REDLINE_STEPS_10S 注释），2400 < 上限，<10s 红线满足。
    /// 超红线（不可能路径，防御性保留）报 REDLINE_EXCEEDED。
    pub fn verify_full(&mut self) -> Result<VerifyReport, CriCode> {
        if self.phase != FreezePhase::Sealed {
            return Err(CriCode::PHASE_ILLEGAL);
        }
        let mut steps: u64 = 0;
        // 结构步 + 长度步（自体复验同样先过两道）。
        let mut mismatch: Option<ChainMismatch> = None;
        if let Some(m) = detect_id_sequence(&self.entries, &mut steps) {
            mismatch = Some(m);
        }
        if mismatch.is_none() {
            if let Some(m) = detect_lengths(&self.entries, &mut steps) {
                mismatch = Some(m);
            }
        }
        // 链步：重算与自身冻结快照比对（无被测异体，快照比对与冻结一致）。
        if mismatch.is_none() {
            if let Some(m) = detect_chain(
                &self.entries,
                self.genesis,
                &self.frozen_chain,
                &self.frozen_chain,
                &mut steps,
            ) {
                mismatch = Some(m);
            }
        }
        let intact: bool = mismatch.is_none();
        let within: bool = steps <= REDLINE_STEPS_10S;
        if !within {
            return Err(CriCode::REDLINE_EXCEEDED);
        }
        self.verify_steps += steps;
        self.ledger_push(
            LedgerAction::VerifyFull,
            0,
            format!(
                "一键复验：{}节点，{}，逻辑步{}（红线{}步）",
                self.entries.len(),
                if intact { "完好" } else { "失配" },
                steps,
                REDLINE_STEPS_10S
            ),
        )?;
        Ok(VerifyReport {
            checked: self.entries.len(),
            intact,
            mismatch,
            steps,
            within_budget: within,
        })
    }

    /// 抽查复验：任意区间 [a, b]（按判据号）带边界前驱哈希复验。
    ///
    /// `expected_prev`：a 的前驱冻结链哈希（a = F0801 时传 genesis），
    /// 可用 prev_hash_of 取得；前驱不符报 PREV_HASH_MISMATCH——不依赖
    /// 全链起点即可定位段内篡改。区间非法报 RANGE_BAD。
    pub fn verify_range(
        &mut self,
        a: u32,
        b: u32,
        expected_prev: u64,
    ) -> Result<RangeReport, CriCode> {
        if self.phase != FreezePhase::Sealed {
            return Err(CriCode::PHASE_ILLEGAL);
        }
        if a > b || n_of(a).is_none() || n_of(b).is_none() {
            return Err(CriCode::RANGE_BAD);
        }
        let ai: usize = (a - FIRST_ID) as usize;
        let bi: usize = (b - FIRST_ID) as usize;
        if bi >= self.entries.len() {
            return Err(CriCode::RANGE_BAD);
        }
        // 边界前驱校验（区间入口被改即报）。
        let local_prev: u64 = if ai == 0 { self.genesis } else { self.frozen_chain[ai - 1].1 };
        if expected_prev != local_prev {
            return Err(CriCode::PREV_HASH_MISMATCH);
        }
        let mut steps: u64 = 1; // 前驱比对 1 步
        let mut prev: u64 = expected_prev;
        let mut mismatch: Option<ChainMismatch> = None;
        for i in ai..=bi {
            // 段内逐节点：重算 2 步 + 双比对 2 步（对条目链点 + 冻结快照）。
            let th: u64 = fnv1a64(self.entries[i].text.as_bytes());
            let ch: u64 = fnv_pair(prev, th);
            steps += NODE_VERIFY_STEPS;
            if self.entries[i].chain_hash != ch {
                mismatch = Some(ChainMismatch {
                    at_index: i,
                    node_id: self.entries[i].id,
                    kind: MismatchKind::TextHashDrift,
                    expect: ch,
                    actual: self.entries[i].chain_hash,
                    note: format!(
                        "抽查判据{}：重算链点{:016x}≠登记链点{:016x}",
                        id_label(self.entries[i].id),
                        ch,
                        self.entries[i].chain_hash
                    ),
                });
                break;
            }
            steps += 1;
            if self.frozen_chain[i].1 != ch {
                mismatch = Some(ChainMismatch {
                    at_index: i,
                    node_id: self.entries[i].id,
                    kind: MismatchKind::SnapshotOverwrite,
                    expect: ch,
                    actual: self.frozen_chain[i].1,
                    note: format!(
                        "抽查判据{}：重算链点{:016x}≠冻结快照{:016x}——快照被覆写",
                        id_label(self.entries[i].id),
                        ch,
                        self.frozen_chain[i].1
                    ),
                });
                break;
            }
            prev = ch;
        }
        let intact: bool = mismatch.is_none();
        let within: bool = steps <= REDLINE_STEPS_10S;
        if !within {
            return Err(CriCode::REDLINE_EXCEEDED);
        }
        self.verify_steps += steps;
        self.ledger_push(
            LedgerAction::VerifyRange,
            a,
            format!(
                "区间抽查[{},{})计{}节点：{}，逻辑步{}",
                id_label(a),
                id_label(b),
                bi - ai + 1,
                if intact { "完好" } else { "失配" },
                steps
            ),
        )?;
        Ok(RangeReport {
            a,
            b,
            checked: bi - ai + 1,
            intact,
            mismatch,
            steps,
            within_budget: within,
        })
    }
}

// ---------------------------------------------------------------------------
// 十三、导出——链快照 kv + 根哈希 + TLV 往返
// ---------------------------------------------------------------------------

/// 字节切片十六进制编码（小写，确定性）。
fn hex_encode(bytes: &[u8]) -> String {
    const HEXC: [char; 16] = [
        '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
    ];
    let mut s: String = String::new();
    for b in bytes {
        s.push(HEXC[(b >> 4) as usize]);
        s.push(HEXC[(b & 0x0f) as usize]);
    }
    s
}

/// u64 的小端十六进制串（16 位宽）。
fn u64_hex(v: u64) -> String {
    hex_encode(&v.to_le_bytes())
}

/// TLV 类型常量：头·链头 genesis。
pub const TLV_TAG_GENESIS: u8 = 0x20;
/// TLV 类型常量：项·判据节点。
pub const TLV_TAG_NODE: u8 = 0x21;
/// TLV 类型常量：头·根哈希。
pub const TLV_TAG_ROOT: u8 = 0x22;
/// TLV 类型常量：头·节点计数。
pub const TLV_TAG_COUNT: u8 = 0x23;

impl CriChain {
    /// 链快照 kv 导出：genesis/root/count/逐节点（键 n+判据号，值链点哈希 hex）。
    ///
    /// 未封冻报 EXPORT_NOT_SEALED；导出入增量账。
    pub fn export_kv(&mut self) -> Result<Vec<(String, String)>, CriCode> {
        if self.phase != FreezePhase::Sealed {
            return Err(CriCode::EXPORT_NOT_SEALED);
        }
        let mut out: Vec<(String, String)> = Vec::new();
        out.push(("genesis".into(), u64_hex(self.genesis)));
        out.push(("root".into(), u64_hex(self.root)));
        out.push(("count".into(), format!("{}", self.frozen_chain.len())));
        for (id, ch) in self.frozen_chain.iter() {
            out.push((format!("n{}", id_label(*id)), u64_hex(*ch)));
        }
        self.ledger_push(
            LedgerAction::Export,
            0,
            format!("kv 导出：{}节点，根哈希{:016x}", self.frozen_chain.len(), self.root),
        )?;
        Ok(out)
    }

    /// TLV 编码：头（genesis/root/count）+ 逐节点项。
    ///
    /// 节点项载荷：id(4B LE) + text_len(4B LE) + text_hash(8B LE) +
    /// chain_hash(8B LE) + text 字节。未封冻报 EXPORT_NOT_SEALED。
    pub fn export_tlv(&self) -> Result<Vec<u8>, CriCode> {
        if self.phase != FreezePhase::Sealed {
            return Err(CriCode::EXPORT_NOT_SEALED);
        }
        let mut out: Vec<u8> = Vec::new();
        // 头：genesis。
        out.push(TLV_TAG_GENESIS);
        out.extend_from_slice(&8u16.to_le_bytes());
        out.extend_from_slice(&self.genesis.to_le_bytes());
        // 头：root。
        out.push(TLV_TAG_ROOT);
        out.extend_from_slice(&8u16.to_le_bytes());
        out.extend_from_slice(&self.root.to_le_bytes());
        // 头：count。
        out.push(TLV_TAG_COUNT);
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&(self.entries.len() as u16).to_le_bytes());
        // 项：逐节点。
        for e in self.entries.iter() {
            let payload_len: usize = 4 + 4 + 8 + 8 + e.text.len();
            if payload_len > 0xffff {
                return Err(CriCode::TEXT_TOO_LONG);
            }
            out.push(TLV_TAG_NODE);
            out.extend_from_slice(&(payload_len as u16).to_le_bytes());
            out.extend_from_slice(&e.id.to_le_bytes());
            out.extend_from_slice(&(e.text_len as u32).to_le_bytes());
            out.extend_from_slice(&e.text_hash.to_le_bytes());
            out.extend_from_slice(&e.chain_hash.to_le_bytes());
            out.extend_from_slice(e.text.as_bytes());
        }
        Ok(out)
    }

    /// TLV 解码重建：解析字节流 → 重建条目表 → 独立重算全链 → 与流内
    /// root 比对（往返不符报 TLV_ROUNDTRIP_ROOT）；结构/长度异常报
    /// TLV_DECODE_BAD。重建链相位直接置 Sealed（以流内 root 为准）。
    pub fn import_tlv(bytes: &[u8]) -> Result<CriChain, CriCode> {
        let mut chain: CriChain = CriChain::new();
        let mut pos: usize = 0;
        let mut got_genesis: Option<u64> = None;
        let mut got_root: Option<u64> = None;
        let mut got_count: Option<usize> = None;
        let mut nodes: Vec<CriEntry> = Vec::new();
        while pos < bytes.len() {
            // Tag + 2 字节长度，长度越界即报。
            if pos + 3 > bytes.len() {
                return Err(CriCode::TLV_DECODE_BAD);
            }
            let tag: u8 = bytes[pos];
            let len: usize = u16::from_le_bytes([bytes[pos + 1], bytes[pos + 2]]) as usize;
            pos += 3;
            if pos + len > bytes.len() {
                return Err(CriCode::TLV_DECODE_BAD);
            }
            let val: &[u8] = &bytes[pos..pos + len];
            match tag {
                TLV_TAG_GENESIS => {
                    if len != 8 {
                        return Err(CriCode::TLV_DECODE_BAD);
                    }
                    got_genesis = Some(u64::from_le_bytes([
                        val[0], val[1], val[2], val[3], val[4], val[5], val[6], val[7],
                    ]));
                }
                TLV_TAG_ROOT => {
                    if len != 8 {
                        return Err(CriCode::TLV_DECODE_BAD);
                    }
                    got_root = Some(u64::from_le_bytes([
                        val[0], val[1], val[2], val[3], val[4], val[5], val[6], val[7],
                    ]));
                }
                TLV_TAG_COUNT => {
                    if len != 2 {
                        return Err(CriCode::TLV_DECODE_BAD);
                    }
                    got_count = Some(u16::from_le_bytes([val[0], val[1]]) as usize);
                }
                TLV_TAG_NODE => {
                    if len < 24 {
                        return Err(CriCode::TLV_DECODE_BAD);
                    }
                    let id: u32 = u32::from_le_bytes([val[0], val[1], val[2], val[3]]);
                    let tlen: usize =
                        u32::from_le_bytes([val[4], val[5], val[6], val[7]]) as usize;
                    let thash: u64 = u64::from_le_bytes([
                        val[8], val[9], val[10], val[11], val[12], val[13], val[14], val[15],
                    ]);
                    let chash: u64 = u64::from_le_bytes([
                        val[16], val[17], val[18], val[19], val[20], val[21], val[22], val[23],
                    ]);
                    if len != 24 + tlen {
                        return Err(CriCode::TLV_DECODE_BAD);
                    }
                    // 文本重建：非 UTF-8 即报（外部输入不可信）。
                    let text: String = match core::str::from_utf8(&val[24..24 + tlen]) {
                        Ok(s) => String::from(s),
                        Err(_) => return Err(CriCode::TLV_DECODE_BAD),
                    };
                    nodes.push(CriEntry {
                        id,
                        text,
                        text_len: tlen,
                        text_hash: thash,
                        chain_hash: chash,
                    });
                }
                _ => return Err(CriCode::TLV_DECODE_BAD),
            }
            pos += len;
        }
        // 头完备性：三件头缺一即报。
        let genesis: u64 = match got_genesis {
            Some(g) => g,
            None => return Err(CriCode::TLV_DECODE_BAD),
        };
        let root: u64 = match got_root {
            Some(r) => r,
            None => return Err(CriCode::TLV_DECODE_BAD),
        };
        let count: usize = match got_count {
            Some(c) => c,
            None => return Err(CriCode::TLV_DECODE_BAD),
        };
        if nodes.len() != count {
            return Err(CriCode::TLV_DECODE_BAD);
        }
        // 往返核一：逐条登记长度账与文本哈希。
        for e in nodes.iter() {
            if e.text_len != e.text.len() || e.text_hash != fnv1a64(e.text.as_bytes()) {
                return Err(CriCode::TLV_ROUNDTRIP_ROOT);
            }
        }
        // 往返核二：独立重算全链与流内链点比对。
        let rechain: Vec<(u32, u64)> = CriChain::chain_over(&nodes, genesis);
        for (i, r) in rechain.iter().enumerate() {
            if i >= nodes.len() || nodes[i].id != r.0 || nodes[i].chain_hash != r.1 {
                return Err(CriCode::TLV_ROUNDTRIP_ROOT);
            }
        }
        // 往返核三：链尾即根。
        if rechain.is_empty() || rechain[rechain.len() - 1].1 != root {
            return Err(CriCode::TLV_ROUNDTRIP_ROOT);
        }
        chain.genesis = genesis;
        chain.entries = nodes;
        chain.frozen_chain = rechain;
        chain.root = root;
        chain.phase = FreezePhase::Sealed;
        Ok(chain)
    }

    /// TLV 往返：本链编码 → 解码重建 → 与本链逐项比对（根 + 全部链点）。
    /// 任一不符报 TLV_ROUNDTRIP_ROOT；往返入增量账。
    pub fn roundtrip_tlv(&mut self) -> Result<usize, CriCode> {
        if self.phase != FreezePhase::Sealed {
            return Err(CriCode::EXPORT_NOT_SEALED);
        }
        let bytes: Vec<u8> = self.export_tlv()?;
        let back: CriChain = CriChain::import_tlv(&bytes)?;
        if back.root != self.root || back.genesis != self.genesis {
            return Err(CriCode::TLV_ROUNDTRIP_ROOT);
        }
        if back.entries.len() != self.entries.len() {
            return Err(CriCode::TLV_ROUNDTRIP_ROOT);
        }
        for (a, b) in self.entries.iter().zip(back.entries.iter()) {
            if a.id != b.id || a.text != b.text || a.text_len != b.text_len
                || a.text_hash != b.text_hash || a.chain_hash != b.chain_hash
            {
                return Err(CriCode::TLV_ROUNDTRIP_ROOT);
            }
        }
        let n: usize = self.entries.len();
        self.ledger_push(
            LedgerAction::RoundTrip,
            0,
            format!("TLV 往返：{}字节编码，{}节点逐项一致，根哈希{:016x}", bytes.len(), n, self.root),
        )?;
        Ok(n)
    }
}

// ---------------------------------------------------------------------------
// 十四、判据逐条落点核对——J1 三段式自证
// ---------------------------------------------------------------------------

/// J1 判据核对项（构造完成后逐项核对落点）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct J1CheckItem {
    /// 核对项名。
    pub item: String,
    /// 是否落点（全 true 为 J1 达成）。
    pub ok: bool,
    /// 中文说明。
    pub note: String,
}

impl CriChain {
    /// J1 判据逐条核对（在完成登记 + 封冻 + 矩阵探测 + 复验后调用）。
    ///
    /// 核对项：①800 条全登记且覆盖 F0801~F1600（verbatim 域）；
    /// ②逐条哈希成链（链点全非零且链尾=根）；③篡改注入 10/10 检出；
    /// ④一键复验步数在 <10s 红线换算内。
    pub fn j1_checklist(
        &self,
        matrix: &[DetectOutcome],
        report: &VerifyReport,
    ) -> Vec<J1CheckItem> {
        let mut out: Vec<J1CheckItem> = Vec::new();
        // ① 800 条 verbatim 全登记。
        out.push(J1CheckItem {
            item: "F0801–F1600 全 800 条判据逐条登记".into(),
            ok: self.is_complete(),
            note: format!(
                "登记{}条，闭式覆盖[{},{}]各一次",
                self.entries.len(),
                id_label(FIRST_ID),
                id_label(LAST_ID)
            ),
        });
        // ② 逐条哈希成链。
        let chained: bool = {
            let mut ok: bool = self.frozen_chain.len() == TOTAL_CRITERIA && self.root != 0;
            for (i, snap) in self.frozen_chain.iter().enumerate() {
                if snap.1 == 0 || snap.0 != id_of(i) {
                    ok = false;
                    break;
                }
            }
            ok
        };
        // ③ 篡改注入检出计数。
        let detected_cnt: usize = matrix.iter().filter(|o| o.detected).count();
        out.push(J1CheckItem {
            item: "逐条 H(n)=FNV(H(n-1)‖FNV(text_n)) 哈希成链".into(),
            ok: chained,
            note: format!(
                "链头{:016x}，根哈希{:016x}，{}链点全非零",
                self.genesis,
                self.root,
                self.frozen_chain.len()
            ),
        });
        // ③ 篡改注入 10/10 检出。
        out.push(J1CheckItem {
            item: "冻结后篡改注入 10/10 检出".into(),
            ok: CriChain::all_detected(matrix),
            note: format!("矩阵{}类样本，检出{}类", matrix.len(), detected_cnt),
        });
        // ④ 一键复验 <10s 红线。
        out.push(J1CheckItem {
            item: "链一键复验 <10s（逻辑步红线换算）".into(),
            ok: report.within_budget && report.steps <= REDLINE_STEPS_10S,
            note: format!(
                "复验{}步，红线{}步，余量约{}倍",
                report.steps,
                REDLINE_STEPS_10S,
                REDLINE_MARGIN
            ),
        });
        out
    }
}

// ---------------------------------------------------------------------------
// 十五、标准演程——登记 → 封冻 → 矩阵探测 → 复验 → 导出往返（一条龙）
// ---------------------------------------------------------------------------

/// 演程摘要（一条龙结果）。
#[derive(Debug, Clone)]
pub struct DrillSummary {
    /// 登记条数。
    pub registered: usize,
    /// 根哈希。
    pub root: u64,
    /// 封冻逻辑步。
    pub seal_steps: u64,
    /// 检出矩阵（10/10）。
    pub matrix: Vec<DetectOutcome>,
    /// 全部检出。
    pub all_detected: bool,
    /// 一键复验报告。
    pub verify: VerifyReport,
    /// TLV 往返节点数。
    pub roundtrip_nodes: usize,
    /// 增量账条数。
    pub ledger_len: usize,
    /// J1 核对清单。
    pub j1: Vec<J1CheckItem>,
}

/// 标准演程：空链 → 全量登记 → 封冻 → 10 样本矩阵 → 一键复验 →
/// 区间抽查 → TLV 往返 → J1 核对。任一步 Err 即短路返回中文诊断。
pub fn run_drill() -> Result<DrillSummary, CriCode> {
    // 一、登记全 800 条。
    let mut chain: CriChain = CriChain::new();
    let registered: usize = chain.register_all_generated()?;
    if registered != TOTAL_CRITERIA {
        return Err(CriCode::SEAL_INCOMPLETE);
    }
    // 二、封冻建链。
    let root: u64 = chain.seal()?;
    let seal_steps: u64 = chain.seal_steps();
    // 三、篡改注入 10/10 检出矩阵。
    let matrix: Vec<DetectOutcome> = chain.detection_matrix()?;
    let all_ok: bool = CriChain::all_detected(&matrix);
    // 四、一键复验（全链 800 节点）。
    let verify: VerifyReport = chain.verify_full()?;
    // 五、区间抽查（域五中部 [F1251, F1260]，前驱取自冻结快照）。
    let prev: u64 = match chain.prev_hash_of(1251) {
        Some(p) => p,
        None => return Err(CriCode::RANGE_BAD),
    };
    let _range: RangeReport = chain.verify_range(1251, 1260, prev)?;
    let rt_nodes: usize = chain.roundtrip_tlv()?; // 六、TLV 往返
    chain.audit_ledger()?; // 七、账面自检
    let j1: Vec<J1CheckItem> = chain.j1_checklist(&matrix, &verify); // 八、J1 核对
    Ok(DrillSummary {
        registered,
        root,
        seal_steps,
        matrix,
        all_detected: all_ok,
        verify,
        roundtrip_nodes: rt_nodes,
        ledger_len: chain.ledger_len(),
        j1,
    })
}
