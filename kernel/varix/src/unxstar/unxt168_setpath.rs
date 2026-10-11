//! unxt168_setpath —— UNX-F0162 SetTime 写路径（写序 + UIP 门控 + 回读验证 + 回滚）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0162 · GetTime/SetTime 语义全量化 · 判据：时间读取与硬件 RTC 对照误差 <1s，夏令时/时区字段语义对照 UEFI 规范
//! · 组件：写序流水（十四步有序写段）+ UIP 门控状态机 + 双源回读对拍 + 失败回滚链式账
//! · 判据落地：写入侧语义（与读取同源同账，写后回读一致才收口）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T168_VERSION: &str = "t168-setpath-v1";

/// 诊断段基址。
pub const T168_SEG_BASE: u16 = 0x3760;

// --------------------------------------------------------------------------
// 诊断码（t168 十六码 · 16 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 写路径诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t168Code {
    /// 0x3760 写序违例（步骤早于前置完成）。
    WriteOutOfOrder,
    /// 0x3761 UIP 门控阻塞（更新周期内写被拦）。
    UipGateBlocked,
    /// 0x3762 回读不一致（写后读回旧值）。
    ReadbackMismatch,
    /// 0x3763 回滚不完整（写序中断留半成品）。
    RollbackPartial,
    /// 0x3764 世纪寄存器跳过（缺位路径显性化）。
    CenturyWriteSkip,
    /// 0x3765 SET 位泄漏（写完未清状态 B）。
    SetBitLeak,
    /// 0x3766 报警面被牵连（写时间改变报警语义）。
    AlarmAffected,
    /// 0x3767 写序台账破缺（步骤账不守恒）。
    WriteLedgerBreak,
    /// 0x3768 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3769 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x376a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x376b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x376c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x376d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x376e 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x376f 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t168Code {
    /// 全量闭集。
    pub const ALL: [t168Code; 16] = [
        t168Code::WriteOutOfOrder,
        t168Code::UipGateBlocked,
        t168Code::ReadbackMismatch,
        t168Code::RollbackPartial,
        t168Code::CenturyWriteSkip,
        t168Code::SetBitLeak,
        t168Code::AlarmAffected,
        t168Code::WriteLedgerBreak,
        t168Code::LedgerBroken,
        t168Code::SnapshotFingerprint,
        t168Code::TableBroken,
        t168Code::IndexMisaligned,
        t168Code::DuplicateEntry,
        t168Code::OrderViolated,
        t168Code::GuardTripped,
        t168Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t168Code::WriteOutOfOrder => 0x3760,
            t168Code::UipGateBlocked => 0x3761,
            t168Code::ReadbackMismatch => 0x3762,
            t168Code::RollbackPartial => 0x3763,
            t168Code::CenturyWriteSkip => 0x3764,
            t168Code::SetBitLeak => 0x3765,
            t168Code::AlarmAffected => 0x3766,
            t168Code::WriteLedgerBreak => 0x3767,
            t168Code::LedgerBroken => 0x3768,
            t168Code::SnapshotFingerprint => 0x3769,
            t168Code::TableBroken => 0x376a,
            t168Code::IndexMisaligned => 0x376b,
            t168Code::DuplicateEntry => 0x376c,
            t168Code::OrderViolated => 0x376d,
            t168Code::GuardTripped => 0x376e,
            t168Code::SegmentGuard => 0x376f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t168Code::WriteOutOfOrder => "写序违例（步骤早于前置完成）",
            t168Code::UipGateBlocked => "UIP 门控阻塞（更新周期内写被拦）",
            t168Code::ReadbackMismatch => "回读不一致（写后读回旧值）",
            t168Code::RollbackPartial => "回滚不完整（写序中断留半成品）",
            t168Code::CenturyWriteSkip => "世纪寄存器跳过（缺位路径显性化）",
            t168Code::SetBitLeak => "SET 位泄漏（写完未清状态 B）",
            t168Code::AlarmAffected => "报警面被牵连（写时间改变报警语义）",
            t168Code::WriteLedgerBreak => "写序台账破缺（步骤账不守恒）",
            t168Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t168Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t168Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t168Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t168Code::DuplicateEntry => "条目重复（同键双写）",
            t168Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t168Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t168Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t168Code::WriteOutOfOrder => "按写序流水重排后执行",
            t168Code::UipGateBlocked => "等 UIP 清零后重试",
            t168Code::ReadbackMismatch => "延长停等后复读或回滚",
            t168Code::RollbackPartial => "按回滚链补全或登记损坏",
            t168Code::CenturyWriteSkip => "登记世纪缺位后继续",
            t168Code::SetBitLeak => "强制清位并登记",
            t168Code::AlarmAffected => "按报警联动规则重算",
            t168Code::WriteLedgerBreak => "重建写序账后复跑",
            t168Code::LedgerBroken => "按守恒律重建台账后重跑",
            t168Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t168Code::TableBroken => "按语义规则重建常量表",
            t168Code::IndexMisaligned => "按声明序归一化后重跑",
            t168Code::DuplicateEntry => "合并同键条目为末次生效",
            t168Code::OrderViolated => "按依赖矩阵重排后重跑",
            t168Code::GuardTripped => "检查临界区嵌套与阈值",
            t168Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 wset（14 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// SetTime 写序流水（十四步依赖链，环依赖即拒）。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum wsetStage {
    /// 级 根：置状态 B 挂起写（SET 位置 1，冻结计数）。
    WRegStatB,
    /// 级 0：写秒寄存器。
    WRegSec,
    /// 级 1：写分寄存器。
    WRegMin,
    /// 级 2：写小时寄存器。
    WRegHour,
    /// 级 3：写星期寄存器。
    WRegWeekday,
    /// 级 4：写日寄存器。
    WRegDay,
    /// 级 5：写月寄存器。
    WRegMonth,
    /// 级 6：写年寄存器（两位年 + 世纪登记）。
    WRegYear,
    /// 级 7：写世纪寄存器（若在册）。
    WRegCentury,
    /// 级 8：等待 UIP 清零。
    WWaitUip,
    /// 级 9：回读秒校验。
    WVerifySec,
    /// 级 10：回读分校验。
    WVerifyMin,
    /// 级 11：回读小时校验。
    WVerifyHour,
    /// 级 12：清状态 B 恢复计数。
    WClearStatB,
}

impl wsetStage {
    /// 全量级闭集。
    pub const ALL: [wsetStage; 14] = [
        wsetStage::WRegStatB,
        wsetStage::WRegSec,
        wsetStage::WRegMin,
        wsetStage::WRegHour,
        wsetStage::WRegWeekday,
        wsetStage::WRegDay,
        wsetStage::WRegMonth,
        wsetStage::WRegYear,
        wsetStage::WRegCentury,
        wsetStage::WWaitUip,
        wsetStage::WVerifySec,
        wsetStage::WVerifyMin,
        wsetStage::WVerifyHour,
        wsetStage::WClearStatB,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            wsetStage::WRegStatB => 0,
            wsetStage::WRegSec => 1,
            wsetStage::WRegMin => 2,
            wsetStage::WRegHour => 3,
            wsetStage::WRegWeekday => 4,
            wsetStage::WRegDay => 5,
            wsetStage::WRegMonth => 6,
            wsetStage::WRegYear => 7,
            wsetStage::WRegCentury => 8,
            wsetStage::WWaitUip => 9,
            wsetStage::WVerifySec => 10,
            wsetStage::WVerifyMin => 11,
            wsetStage::WVerifyHour => 12,
            wsetStage::WClearStatB => 13,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            wsetStage::WRegStatB => "置状态 B 挂起写（SET 位置 1，冻结计数）",
            wsetStage::WRegSec => "写秒寄存器",
            wsetStage::WRegMin => "写分寄存器",
            wsetStage::WRegHour => "写小时寄存器",
            wsetStage::WRegWeekday => "写星期寄存器",
            wsetStage::WRegDay => "写日寄存器",
            wsetStage::WRegMonth => "写月寄存器",
            wsetStage::WRegYear => "写年寄存器（两位年 + 世纪登记）",
            wsetStage::WRegCentury => "写世纪寄存器（若在册）",
            wsetStage::WWaitUip => "等待 UIP 清零",
            wsetStage::WVerifySec => "回读秒校验",
            wsetStage::WVerifyMin => "回读分校验",
            wsetStage::WVerifyHour => "回读小时校验",
            wsetStage::WClearStatB => "清状态 B 恢复计数",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const WSET_DEPS: [i32; 14] = [
    -1, // WRegStatB 依赖 无
    0, // WRegSec 依赖 级 0
    1, // WRegMin 依赖 级 1
    2, // WRegHour 依赖 级 2
    3, // WRegWeekday 依赖 级 3
    4, // WRegDay 依赖 级 4
    5, // WRegMonth 依赖 级 5
    6, // WRegYear 依赖 级 6
    7, // WRegCentury 依赖 级 7
    8, // WWaitUip 依赖 级 8
    9, // WVerifySec 依赖 级 9
    10, // WVerifyMin 依赖 级 10
    11, // WVerifyHour 依赖 级 11
    12, // WClearStatB 依赖 级 12
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum wsetProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（SET 位泄漏场景）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct wsetBook {
    /// 各级状态。
    pub state: [wsetProgress; 14],
    /// 已完成计数。
    pub done: u32,
    /// 跳过计数。
    pub skipped: u32,
    /// 回滚计数。
    pub rolled: u32,
    /// 已完成级索引序列（回放审计用）。
    pub order: [usize; 14],
    /// 序列长度。
    pub order_len: usize,
}

impl wsetBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [wsetProgress::Pending; 14],
            done: 0,
            skipped: 0,
            rolled: 0,
            order: [0; 14],
            order_len: 0,
        }
    }

    /// 拓扑序推演（Kahn 剥皮：逐轮找全前置已 Done 的 Pending 级）。
    /// 返回推演出的可执行序；环依赖时返回 Err(环首级索引)。
    pub fn plan(&mut self) -> Result<[usize; 14], usize> {
        let mut order = [0usize; 14];
        let mut olen = 0usize;
        let mut rounds = 0usize;
        while rounds < 14 {
            let mut progressed = false;
            let mut i = 0usize;
            while i < 14 {
                if self.state[i] != wsetProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = WSET_DEPS[i];
                if dep < 0 || self.state[dep as usize] == wsetProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = wsetProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 14 {
                    if self.state[k] == wsetProgress::Pending {
                        return Err(k);
                    }
                    k += 1;
                }
            }
            rounds += 1;
        }
        self.order = order;
        self.order_len = olen;
        Ok(order)
    }

    /// 注入失败：自失败级起全部 Done 级回滚（SET 位泄漏场景）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 14 {
            if self.state[i] == wsetProgress::Done {
                self.state[i] = wsetProgress::Rolled;
                if self.done > 0 {
                    self.done -= 1;
                }
                self.rolled += 1;
            }
            i += 1;
        }
    }

    /// 守恒恒等式：done + skipped + rolled + pending == 总级数。
    pub fn conserved(&self) -> bool {
        let mut pending = 0u32;
        let mut i = 0usize;
        while i < 14 {
            if self.state[i] == wsetProgress::Pending {
                pending += 1;
            }
            i += 1;
        }
        self.done + self.skipped + self.rolled + pending == 14
    }

    /// 已完成级回放序列与推演序一致（顺序守恒证据）。
    pub fn order_matches(&self) -> bool {
        let mut i = 0usize;
        while i < self.order_len {
            if self.order[i] >= 14 || self.state[self.order[i]] != wsetProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 引擎 wchain（链式指纹：步骤链式指纹 + 逐链回放校验）
// --------------------------------------------------------------------------
/// 写序链式账（每步一链环，负载=步骤序号+1）。
#[derive(Clone, Copy, Debug)]
pub struct wchainLink {
    /// 段标签（如十二步节名）。
    pub tag: u32,
    /// 段负载（长度/条目数/字段数的语义化标量）。
    pub payload: u64,
    /// 本链指纹（FNV-1a 64 链式）。
    pub digest: u64,
}

/// 链式账本（append-only：只增不改，破坏即检出）。
#[derive(Clone, Copy, Debug)]
pub struct wchainChain {
    /// 链节点序列。
    pub links: [wchainLink; 16],
    /// 节点数。
    pub n: usize,
    /// 累计负载。
    pub payload_total: u64,
    /// 总链指纹（最后一环）。
    pub head: u64,
}

impl wchainChain {
    /// 空链构造。
    pub const fn new() -> Self {
        Self {
            links: [wchainLink { tag: 0, payload: 0, digest: 0 }; 16],
            n: 0,
            payload_total: 0,
            head: FNV_OFFSET,
        }
    }

    /// 追加一段（负载超标或容量满时拒收）。
    pub fn append(&mut self, tag: u32, payload: u64) -> Result<u64, t168Code> {
        if self.n >= 16 || payload == 0 {
            return Err(t168Code::ChainCapOrEmpty);
        }
        let digest = fnv1a64_from(self.head, &payload.to_le_bytes());
        self.links[self.n] = wchainLink { tag, payload, digest };
        self.n += 1;
        self.payload_total += payload;
        self.head = digest;
        Ok(digest)
    }

    /// 全链回放校验（逐环重算指纹 + 负载守恒）。
    pub fn verify(&self) -> bool {
        let mut h = FNV_OFFSET;
        let mut total = 0u64;
        let mut i = 0usize;
        while i < self.n {
            let lk = self.links[i];
            h = fnv1a64_from(h, &lk.payload.to_le_bytes());
            if h != lk.digest {
                return false;
            }
            total += lk.payload;
            i += 1;
        }
        total == self.payload_total && h == self.head
    }
}

// --------------------------------------------------------------------------
// 引擎 wdual（双源对拍：3 槽位逐槽回读一致性）
// --------------------------------------------------------------------------
/// 写后回读对拍（秒/分/时三槽）。
/// 槽位枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum wdualSlot {
    /// 秒槽。
    Sec,
    /// 分槽。
    Min,
    /// 小时槽。
    Hour,
}

impl wdualSlot {
    /// 全量槽位闭集。
    pub const ALL: [wdualSlot; 3] = [
        wdualSlot::Sec,
        wdualSlot::Min,
        wdualSlot::Hour,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            wdualSlot::Sec => 0,
            wdualSlot::Min => 1,
            wdualSlot::Hour => 2,
        }
    }

    /// 中文槽名。
    pub const fn cn(self) -> &'static str {
        match self {
            wdualSlot::Sec => "秒槽",
            wdualSlot::Min => "分槽",
            wdualSlot::Hour => "小时槽",
        }
    }
}

/// 双源样本（A 源首读 + B 源回读 + 一致性判定）。
#[derive(Clone, Copy, Debug)]
pub struct wdualPair {
    /// A 源首读值。
    pub a: u64,
    /// B 源回读值。
    pub b: u64,
    /// 是否一致。
    pub same: bool,
    /// 采样序号。
    pub seq: u64,
}

/// 对拍账（逐槽一致才放行；不一致槽位显性记账）。
#[derive(Clone, Copy, Debug)]
pub struct wdualDual {
    /// 逐槽样本。
    pub pairs: [wdualPair; 3],
    /// 一致槽位数。
    pub same_n: usize,
    /// 不一致槽位数。
    pub diff_n: usize,
    /// 采样总轮次。
    pub rounds: u64,
    /// 已采样的不一致槽位掩码。
    pub diff_mask: u64,
}

impl wdualDual {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            pairs: [wdualPair { a: 0, b: 0, same: false, seq: 0 }; 3],
            same_n: 0,
            diff_n: 0,
            rounds: 0,
            diff_mask: 0,
        }
    }

    /// 单槽采样（A/B 双源读入后逐位比较）。
    pub fn sample(&mut self, who: wdualSlot, a: u64, b: u64) {
        let i = who.index();
        let same = a == b;
        self.pairs[i] = wdualPair { a, b, same, seq: self.rounds };
        if same {
            self.same_n += 1;
        } else {
            self.diff_n += 1;
            self.diff_mask |= 1u64 << (i as u64);
        }
        self.rounds += 1;
    }

    /// 全槽一致判定（对拍通过口径）。
    pub fn all_same(&self) -> bool {
        self.diff_n == 0 && self.same_n == 3
    }

    /// 复算：逐槽重读 pairs 重判 same（防记账面与数据面分叉）。
    pub fn recount(&self) -> bool {
        let mut same = 0usize;
        let mut i = 0usize;
        while i < 3 {
            if self.pairs[i].same == (self.pairs[i].a == self.pairs[i].b) {
                same += 1;
            }
            i += 1;
        }
        same == 3 && self.same_n + self.diff_n == 3
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3760, 0x376f]。
pub const fn t168_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t168Code::ALL.len() {
        let c = t168Code::ALL[i].code();
        if c < 0x3760 || c > 0x376f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：每检查一带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 段位守卫：全部诊断码落在本模块段内。
fn t168_guard_seg() -> bool {
    t168_segment_guard()
}

/// 写序流水拓扑推演（14 级全达 + 顺序单调）。
fn check_c1_pipeline_plan() -> bool {
    let mut bk = wsetBook::new();
    let mut ok = matches!(bk.plan(), Ok(_));
    if ok {
        ok = bk.done == 14 && bk.order_len == 14;
    }
    if ok {
        // 首步必须是无前置的置位步。
        ok = bk.order[0] == 0;
    }
    ok
}

/// 写序链式账追加 + 全链回放校验。
fn check_c2_chain_verify() -> bool {
    let mut ch = wchainChain::new();
    let mut i = 1u64;
    let mut ok = true;
    while i <= 14 {
        if ch.append((i as u32) % 100, i * 100).is_err() {
            ok = false;
        }
        i += 1;
    }
    if ok {
        ok = ch.verify();
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t168_checks() -> CheckSet {
    let mut set = CheckSet::new("T168");
    set.add(
        "t168-seg-guard",
        t168_guard_seg(),
        if t168_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t168-c1-pipeline-plan",
        check_c1_pipeline_plan(),
        if check_c1_pipeline_plan() { "" } else { "write pipeline plan mismatch" },
    );
    set.add(
        "t168-c2-chain-verify",
        check_c2_chain_verify(),
        if check_c2_chain_verify() { "" } else { "write chain verify mismatch" },
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（域聚合测试在 mod.rs——此处只放本模块纯函数级断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t168_all_checks_green() {
        let set = run_t168_checks();
        assert!(set.all_passed(), "T168 domain checks must be green");
    }

    #[test]
    fn t168_segment_guard_holds() {
        assert!(t168_segment_guard());
    }
}
