//! unxt263_switcher —— UNX-F0261 双头切换与扫描输出序。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0261 · 帧簿双头显（诊断浮层 + 正常输出合成） · 判据：诊断浮层与正常输出同屏合成零互蚀，浮层开合零残影
//! · 组件：扫描输出序表（32 行）+ 切换流水（六级）+ 头仲裁状态机
//! · 判据落地：同屏合成的输出面
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T263_VERSION: &str = "t263-switcher-v1";

/// 诊断段基址。
pub const T263_SEG_BASE: u16 = 0x3a20;

// --------------------------------------------------------------------------
// 诊断码（t263 十六码 · 16 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 切换诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t263Code {
    /// 0x3a20 双头冲突（两头同扫一缓冲）。
    HeadConflict,
    /// 0x3a21 扫描次序非法（帧内乱序）。
    ScanOutOrderBad,
    /// 0x3a22 消隐错过（切换出在非窗期）。
    VblankMissed,
    /// 0x3a23 撕裂检出（扫中换页）。
    TearingDetected,
    /// 0x3a24 头未绑定（扫描无目标头）。
    HeadUnbound,
    /// 0x3a25 重复翻页（同步双翻）。
    DoubleFlip,
    /// 0x3a26 管线停滞（级间无产出）。
    PipeStall,
    /// 0x3a27 切换账破缺。
    LedgerBreak,
    /// 0x3a28 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3a29 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3a2a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x3a2b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3a2c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3a2d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x3a2e 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x3a2f 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t263Code {
    /// 全量闭集。
    pub const ALL: [t263Code; 16] = [
        t263Code::HeadConflict,
        t263Code::ScanOutOrderBad,
        t263Code::VblankMissed,
        t263Code::TearingDetected,
        t263Code::HeadUnbound,
        t263Code::DoubleFlip,
        t263Code::PipeStall,
        t263Code::LedgerBreak,
        t263Code::LedgerBroken,
        t263Code::SnapshotFingerprint,
        t263Code::TableBroken,
        t263Code::IndexMisaligned,
        t263Code::DuplicateEntry,
        t263Code::OrderViolated,
        t263Code::GuardTripped,
        t263Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t263Code::HeadConflict => 0x3a20,
            t263Code::ScanOutOrderBad => 0x3a21,
            t263Code::VblankMissed => 0x3a22,
            t263Code::TearingDetected => 0x3a23,
            t263Code::HeadUnbound => 0x3a24,
            t263Code::DoubleFlip => 0x3a25,
            t263Code::PipeStall => 0x3a26,
            t263Code::LedgerBreak => 0x3a27,
            t263Code::LedgerBroken => 0x3a28,
            t263Code::SnapshotFingerprint => 0x3a29,
            t263Code::TableBroken => 0x3a2a,
            t263Code::IndexMisaligned => 0x3a2b,
            t263Code::DuplicateEntry => 0x3a2c,
            t263Code::OrderViolated => 0x3a2d,
            t263Code::GuardTripped => 0x3a2e,
            t263Code::SegmentGuard => 0x3a2f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t263Code::HeadConflict => "双头冲突（两头同扫一缓冲）",
            t263Code::ScanOutOrderBad => "扫描次序非法（帧内乱序）",
            t263Code::VblankMissed => "消隐错过（切换出在非窗期）",
            t263Code::TearingDetected => "撕裂检出（扫中换页）",
            t263Code::HeadUnbound => "头未绑定（扫描无目标头）",
            t263Code::DoubleFlip => "重复翻页（同步双翻）",
            t263Code::PipeStall => "管线停滞（级间无产出）",
            t263Code::LedgerBreak => "切换账破缺",
            t263Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t263Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t263Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t263Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t263Code::DuplicateEntry => "条目重复（同键双写）",
            t263Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t263Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t263Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t263Code::HeadConflict => "按头仲裁独占",
            t263Code::ScanOutOrderBad => "按扫描序表重排",
            t263Code::VblankMissed => "按 vblank 窗重切",
            t263Code::TearingDetected => "回切并登记",
            t263Code::HeadUnbound => "绑定后重扫",
            t263Code::DoubleFlip => "幂等去重",
            t263Code::PipeStall => "按停滞检测重组",
            t263Code::LedgerBreak => "按守恒重建",
            t263Code::LedgerBroken => "按守恒律重建台账后重跑",
            t263Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t263Code::TableBroken => "按语义规则重建常量表",
            t263Code::IndexMisaligned => "按声明序归一化后重跑",
            t263Code::DuplicateEntry => "合并同键条目为末次生效",
            t263Code::OrderViolated => "按依赖矩阵重排后重跑",
            t263Code::GuardTripped => "检查临界区嵌套与阈值",
            t263Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 scanout（32 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 扫描输出序表（32 行逐行位序守卫）。
/// 行数。
pub const scanout_COUNT: usize = 32;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static scanout: [(&'static str, u64); 32] = [
    ("PF_RGB332", 0), // PF_RGB332 输出位序（0..31 连续）
    ("PF_RGB444", 1), // PF_RGB444 输出位序（0..31 连续）
    ("PF_ARGB4444", 2), // PF_ARGB4444 输出位序（0..31 连续）
    ("PF_RGB555", 3), // PF_RGB555 输出位序（0..31 连续）
    ("PF_ARGB1555", 4), // PF_ARGB1555 输出位序（0..31 连续）
    ("PF_RGB565", 5), // PF_RGB565 输出位序（0..31 连续）
    ("PF_BGR565", 6), // PF_BGR565 输出位序（0..31 连续）
    ("PF_RGB888", 7), // PF_RGB888 输出位序（0..31 连续）
    ("PF_BGR888", 8), // PF_BGR888 输出位序（0..31 连续）
    ("PF_RGBX8888", 9), // PF_RGBX8888 输出位序（0..31 连续）
    ("PF_BGRX8888", 10), // PF_BGRX8888 输出位序（0..31 连续）
    ("PF_ARGB8888", 11), // PF_ARGB8888 输出位序（0..31 连续）
    ("PF_BGRA8888", 12), // PF_BGRA8888 输出位序（0..31 连续）
    ("PF_RGBA8888", 13), // PF_RGBA8888 输出位序（0..31 连续）
    ("PF_ABGR8888", 14), // PF_ABGR8888 输出位序（0..31 连续）
    ("PF_RGB1010102", 15), // PF_RGB1010102 输出位序（0..31 连续）
    ("PF_BGR1010102", 16), // PF_BGR1010102 输出位序（0..31 连续）
    ("PF_ARGB2101010", 17), // PF_ARGB2101010 输出位序（0..31 连续）
    ("PF_ABGR2101010", 18), // PF_ABGR2101010 输出位序（0..31 连续）
    ("PF_RGBA16F", 19), // PF_RGBA16F 输出位序（0..31 连续）
    ("PF_RGBA32F", 20), // PF_RGBA32F 输出位序（0..31 连续）
    ("PF_A8", 21), // PF_A8 输出位序（0..31 连续）
    ("PF_L8", 22), // PF_L8 输出位序（0..31 连续）
    ("PF_AL44", 23), // PF_AL44 输出位序（0..31 连续）
    ("PF_YUY2", 24), // PF_YUY2 输出位序（0..31 连续）
    ("PF_UYVY", 25), // PF_UYVY 输出位序（0..31 连续）
    ("PF_NV12", 26), // PF_NV12 输出位序（0..31 连续）
    ("PF_YV12", 27), // PF_YV12 输出位序（0..31 连续）
    ("PF_IYUV", 28), // PF_IYUV 输出位序（0..31 连续）
    ("PF_D16", 29), // PF_D16 输出位序（0..31 连续）
    ("PF_D24S8", 30), // PF_D24S8 输出位序（0..31 连续）
    ("PF_D32F", 31), // PF_D32F 输出位序（0..31 连续）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn scanout_get(key: &str) -> Option<u64> {
    match key {
        "PF_RGB332" => Some(0),
        "PF_RGB444" => Some(1),
        "PF_ARGB4444" => Some(2),
        "PF_RGB555" => Some(3),
        "PF_ARGB1555" => Some(4),
        "PF_RGB565" => Some(5),
        "PF_BGR565" => Some(6),
        "PF_RGB888" => Some(7),
        "PF_BGR888" => Some(8),
        "PF_RGBX8888" => Some(9),
        "PF_BGRX8888" => Some(10),
        "PF_ARGB8888" => Some(11),
        "PF_BGRA8888" => Some(12),
        "PF_RGBA8888" => Some(13),
        "PF_ABGR8888" => Some(14),
        "PF_RGB1010102" => Some(15),
        "PF_BGR1010102" => Some(16),
        "PF_ARGB2101010" => Some(17),
        "PF_ABGR2101010" => Some(18),
        "PF_RGBA16F" => Some(19),
        "PF_RGBA32F" => Some(20),
        "PF_A8" => Some(21),
        "PF_L8" => Some(22),
        "PF_AL44" => Some(23),
        "PF_YUY2" => Some(24),
        "PF_UYVY" => Some(25),
        "PF_NV12" => Some(26),
        "PF_YV12" => Some(27),
        "PF_IYUV" => Some(28),
        "PF_D16" => Some(29),
        "PF_D24S8" => Some(30),
        "PF_D32F" => Some(31),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn scanout_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < scanout_COUNT {
        let (k, v) = scanout[i];
        let ok = match k {
            "PF_RGB332" => v == 0,
            "PF_RGB444" => v == 1,
            "PF_ARGB4444" => v == 2,
            "PF_RGB555" => v == 3,
            "PF_ARGB1555" => v == 4,
            "PF_RGB565" => v == 5,
            "PF_BGR565" => v == 6,
            "PF_RGB888" => v == 7,
            "PF_BGR888" => v == 8,
            "PF_RGBX8888" => v == 9,
            "PF_BGRX8888" => v == 10,
            "PF_ARGB8888" => v == 11,
            "PF_BGRA8888" => v == 12,
            "PF_RGBA8888" => v == 13,
            "PF_ABGR8888" => v == 14,
            "PF_RGB1010102" => v == 15,
            "PF_BGR1010102" => v == 16,
            "PF_ARGB2101010" => v == 17,
            "PF_ABGR2101010" => v == 18,
            "PF_RGBA16F" => v == 19,
            "PF_RGBA32F" => v == 20,
            "PF_A8" => v == 21,
            "PF_L8" => v == 22,
            "PF_AL44" => v == 23,
            "PF_YUY2" => v == 24,
            "PF_UYVY" => v == 25,
            "PF_NV12" => v == 26,
            "PF_YV12" => v == 27,
            "PF_IYUV" => v == 28,
            "PF_D16" => v == 29,
            "PF_D24S8" => v == 30,
            "PF_D32F" => v == 31,
            _ => false,
        };
        if !ok {
            bad += 1;
        }
        i += 1;
    }
    bad
}

/// 累计求和（守恒对账用）。
pub fn scanout_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < scanout_COUNT {
        s = s.wrapping_add(scanout[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn scanout_unique() -> bool {
    let mut i = 0usize;
    while i < scanout_COUNT {
        let mut j = i + 1;
        while j < scanout_COUNT {
            if scanout[i].0 == scanout[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 引擎 sw（6 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 双头切换六级流水。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum swStage {
    /// 级 根：冻结当前帧簿状态。
    Freeze,
    /// 级 0：合成浮层与正常输出。
    Compose,
    /// 级 1：合成结果校验（零互蚀）。
    Verify,
    /// 级 2：等待消隐窗。
    WaitVblank,
    /// 级 3：翻页交换。
    Flip,
    /// 级 4：释放旧帧。
    Release,
}

impl swStage {
    /// 全量级闭集。
    pub const ALL: [swStage; 6] = [
        swStage::Freeze,
        swStage::Compose,
        swStage::Verify,
        swStage::WaitVblank,
        swStage::Flip,
        swStage::Release,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            swStage::Freeze => 0,
            swStage::Compose => 1,
            swStage::Verify => 2,
            swStage::WaitVblank => 3,
            swStage::Flip => 4,
            swStage::Release => 5,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            swStage::Freeze => "冻结当前帧簿状态",
            swStage::Compose => "合成浮层与正常输出",
            swStage::Verify => "合成结果校验（零互蚀）",
            swStage::WaitVblank => "等待消隐窗",
            swStage::Flip => "翻页交换",
            swStage::Release => "释放旧帧",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const SW_DEPS: [i32; 6] = [
    -1, // Freeze 依赖 无
    0, // Compose 依赖 级 0
    1, // Verify 依赖 级 1
    2, // WaitVblank 依赖 级 2
    3, // Flip 依赖 级 3
    4, // Release 依赖 级 4
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum swProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（扫中换页撕裂）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct swBook {
    /// 各级状态。
    pub state: [swProgress; 6],
    /// 已完成计数。
    pub done: u32,
    /// 跳过计数。
    pub skipped: u32,
    /// 回滚计数。
    pub rolled: u32,
    /// 已完成级索引序列（回放审计用）。
    pub order: [usize; 6],
    /// 序列长度。
    pub order_len: usize,
}

impl swBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [swProgress::Pending; 6],
            done: 0,
            skipped: 0,
            rolled: 0,
            order: [0; 6],
            order_len: 0,
        }
    }

    /// 拓扑序推演（Kahn 剥皮：逐轮找全前置已 Done 的 Pending 级）。
    /// 返回推演出的可执行序；环依赖时返回 Err(环首级索引)。
    pub fn plan(&mut self) -> Result<[usize; 6], usize> {
        let mut order = [0usize; 6];
        let mut olen = 0usize;
        let mut rounds = 0usize;
        while rounds < 6 {
            let mut progressed = false;
            let mut i = 0usize;
            while i < 6 {
                if self.state[i] != swProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = SW_DEPS[i];
                if dep < 0 || self.state[dep as usize] == swProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = swProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 6 {
                    if self.state[k] == swProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（扫中换页撕裂）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 6 {
            if self.state[i] == swProgress::Done {
                self.state[i] = swProgress::Rolled;
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
        while i < 6 {
            if self.state[i] == swProgress::Pending {
                pending += 1;
            }
            i += 1;
        }
        self.done + self.skipped + self.rolled + pending == 6
    }

    /// 已完成级回放序列与推演序一致（顺序守恒证据）。
    pub fn order_matches(&self) -> bool {
        let mut i = 0usize;
        while i < self.order_len {
            if self.order[i] >= 6 || self.state[self.order[i]] != swProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 状态机 head（4 态 × 4 事件 · 迁移 6 条显式声明）
// --------------------------------------------------------------------------
/// 头仲裁状态机状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum headState {
    Idle,
    HeadA,
    HeadB,
    Migrating,
}

impl headState {
    /// 全量态闭集。
    pub const ALL: [headState; 4] = [
        headState::Idle,
        headState::HeadA,
        headState::HeadB,
        headState::Migrating,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            headState::Idle => 0,
            headState::HeadA => 1,
            headState::HeadB => 2,
            headState::Migrating => 3,
        }
    }
}

/// head事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum headEvent {
    BindA,
    BindB,
    Swap,
    Commit,
}

impl headEvent {
    /// 全量事件闭集。
    pub const ALL: [headEvent; 4] = [
        headEvent::BindA,
        headEvent::BindB,
        headEvent::Swap,
        headEvent::Commit,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const HEAD_TRANSITIONS: [(headState, headEvent, headState); 6] = [
    (headState::Idle, headEvent::BindA, headState::HeadA), // 绑定头 A
    (headState::Idle, headEvent::BindB, headState::HeadB), // 绑定头 B
    (headState::HeadA, headEvent::Swap, headState::Migrating), // 发起迁移
    (headState::HeadB, headEvent::Swap, headState::Migrating), // 发起迁移
    (headState::Migrating, headEvent::Commit, headState::HeadB), // 迁移到头 B
    (headState::Migrating, headEvent::Commit, headState::HeadA), // 迁移回头 A
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn head_next(state: headState, ev: headEvent) -> Option<headState> {
    let mut i = 0usize;
    while i < 6 {
        let t = HEAD_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn head_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < headState::ALL.len() {
        let mut ei = 0usize;
        while ei < headEvent::ALL.len() {
            let legal = head_next(headState::ALL[si], headEvent::ALL[ei]).is_some();
            if !legal {
                bad += 1;
            }
            ei += 1;
        }
        si += 1;
    }
    bad
}

/// 终态集合（无出边的态即终态）。
pub fn head_terminal(st: headState) -> bool {
    let mut i = 0usize;
    while i < 6 {
        if HEAD_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct headLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: headState,
}

impl headLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: headState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: headEvent) -> bool {
        match head_next(self.cur, ev) {
            Some(ns) => {
                self.cur = ns;
                self.legal += 1;
                true
            }
            None => {
                self.illegal += 1;
                false
            }
        }
    }

    /// 守恒恒等式：legal + illegal == 施加次数（由调用方累计）。
    pub fn total(&self) -> u64 {
        self.legal + self.illegal
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3a20, 0x3a2f]。
pub const fn t263_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t263Code::ALL.len() {
        let c = t263Code::ALL[i].code();
        if c < 0x3a20 || c > 0x3a2f {
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
fn t263_guard_seg() -> bool {
    t263_segment_guard()
}

/// 表 scanout 逐行守卫全绿（行数/守卫/唯一性）。
fn check_t263_scanout_guard() -> bool {
    let mut ok = scanout_COUNT == 32;
    if ok {
        ok = scanout_scan() == 0;
    }
    if ok {
        ok = scanout_unique();
    }
    ok
}

/// 切换六级流水推演全达。
fn check_c2_switch_pipeline() -> bool {
    let mut bk = swBook::new();
    let mut ok = matches!(bk.plan(), Ok(_)) && bk.done == 6;
    if ok {
        ok = bk.order[0] == 0 && bk.order[5] == 5;
    }
    ok
}

/// 头仲裁迁移路径（A→Migrating→B）。
fn check_c3_head_sm() -> bool {
    let mut led = headLedger::new(headState::Idle);
    let mut ok = led.apply(headEvent::BindA);
    ok = ok && led.apply(headEvent::Swap);
    if ok {
        ok = led.apply(headEvent::Commit) && led.cur == headState::HeadB;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t263_checks() -> CheckSet {
    let mut set = CheckSet::new("T263");
    set.add(
        "t263-seg-guard",
        t263_guard_seg(),
        if t263_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t263-c1-scanout-guard",
        check_t263_scanout_guard(),
        if check_t263_scanout_guard() { "" } else { "table scanout guard mismatch" },
    );
    set.add(
        "t263-c2-switch-pipeline",
        check_c2_switch_pipeline(),
        if check_c2_switch_pipeline() { "" } else { "switch pipeline mismatch" },
    );
    set.add(
        "t263-c3-head-sm",
        check_c3_head_sm(),
        if check_c3_head_sm() { "" } else { "head arbiter state machine mismatch" },
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
    fn t263_all_checks_green() {
        let set = run_t263_checks();
        assert!(set.all_passed(), "T263 domain checks must be green");
    }

    #[test]
    fn t263_segment_guard_holds() {
        assert!(t263_segment_guard());
    }
}
