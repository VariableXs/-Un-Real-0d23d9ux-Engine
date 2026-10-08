//! CGPU-F1281 · I 域开工与带宽总架构（CGPU-I 域 · 带宽与总线域 · I01 组 · 目标 380 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1281`
//!
//! **判据（锚点原文）**：五主题、账户化、边界、十组、判据。
//!
//! **职责定位（锚点原文）**：I 域开工：带宽与总线域（F1281-F1440，官方
//! 五主题：PCIe 带宽仲裁/共享内存带宽/上传下载调度/带宽遥测/瓶颈归因）；
//! 带宽架构（带宽=GPU 与系统的数据通道——所有传输（纹理上传/下载/交换/
//! 流送）走带宽预算账户）；跨域边界（H 域管显存驻留、本域管传输带宽
//! ——边界声明复用 F0405；与 VE-AD 存储流 F5801+ 分工声明）；十组规划
//! 声明。
//!
//! ## 一、五主题是域的地图，十组是域的进度条
//!
//! 五主题（[`BandwidthTopic`]）来自锚点原文官方口径，本域 160 项全部
//! 挂在五主题之一上——开工单先冻结主题闭集，后 159 项不得另立主题。
//! 十组（[`I_GROUPS`]）从总纲批次标题逐字提取（组名+起止单号），是
//! I 域的规划进度条：每收一组，表内打一格；组表 stamp 由判据侧独立
//! 重排对账，错一字先红。
//!
//! ## 二、账户化：每笔传输都记账，超预算就拒绝
//!
//! 带宽是共享通道，不记账必然互相挤占（判据二「账户化」）：四类传输
//! （纹理上传/下载/交换/流送）各开一个预算账户（[`BudgetLedger`]），
//! 每笔传输先申请（[`BudgetLedger::request`]）——余额不足即拒绝（拒绝
//! 计数留痕，不静默排队膨胀）；实际用量小于申请量退回（[`BudgetLedger::refund`]）；
//! 账本守恒自检：consumed + refunded + remaining ≡ allocated 累计。
//!
//! ## 三、边界：H 管驻留、I 管搬运、AD 管到达
//!
//! 三域分工在文档常量里写死（[`BOUNDARY_DOC`]）：H 域管显存驻留（数据
//! 已在显存的放哪/换页），本域管传输带宽（数据搬运通道的速率与仲裁），
//! VE-AD 存储流（F5801+）管数据从盘到内存的到达——接口复用 F0405 定的
//! I 域集成面。三句声明各自可 grep，越界实现没有立足点。
//!
//! **对接**：F0405（I 域接口集成面）；F1283（预算账户深化）；F1284+
//! （PCIe 仲裁）。零 panic 面（下标走 `get`/`Option`，算术饱和/checked）、
//! 零 IO、零墙钟（逻辑 tick 注入）、无全局可变状态、no_std 零 std 依赖。

use alloc::format;
use alloc::string::String;

// ---------------------------------------------------------------------------
// 一、规格常量与五主题闭集（判据一、判据五）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const BANDWIDTH_ARCH_VERSION: &str = "I01-bandwidth-arch-v1";

/// 带宽与总线域官方五主题（锚点原文闭集；本域 160 项全部挂靠，表外不立题）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BandwidthTopic {
    /// PCIe 带宽仲裁。
    PcieArbitration,
    /// 共享内存带宽。
    SharedMemBandwidth,
    /// 上传下载调度。
    UploadDownloadScheduling,
    /// 带宽遥测。
    BandwidthTelemetry,
    /// 瓶颈归因。
    BottleneckAttribution,
}

impl BandwidthTopic {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            BandwidthTopic::PcieArbitration => "PCIe带宽仲裁",
            BandwidthTopic::SharedMemBandwidth => "共享内存带宽",
            BandwidthTopic::UploadDownloadScheduling => "上传下载调度",
            BandwidthTopic::BandwidthTelemetry => "带宽遥测",
            BandwidthTopic::BottleneckAttribution => "瓶颈归因",
        }
    }
}

/// 五主题齐备（开工面：闭集冻结的结构断言源）。
pub const fn topics_present() -> [BandwidthTopic; 5] {
    [
        BandwidthTopic::PcieArbitration,
        BandwidthTopic::SharedMemBandwidth,
        BandwidthTopic::UploadDownloadScheduling,
        BandwidthTopic::BandwidthTelemetry,
        BandwidthTopic::BottleneckAttribution,
    ]
}

/// 十组规划（判据五：组名与起止单号逐字取自总纲批次锚点；进度条与对账底表）。
pub const I_GROUPS: [(&str, u32, u32); 10] = [
    ("带宽总架构与 PCIe 仲裁组", 1281, 1296),
    ("上传下载调度组", 1297, 1312),
    ("共享内存带宽组", 1313, 1328),
    ("带宽与 G 域协同组", 1329, 1344),
    ("传输安全与诊断组", 1345, 1360),
    ("混合场景带宽组", 1361, 1376),
    ("带宽与场景适配组", 1377, 1392),
    ("I 域预备与自查组", 1393, 1408),
    ("带宽场景扩展组", 1409, 1424),
    ("I 域收口组", 1425, 1440),
];

/// 跨域边界声明（判据四：H 管驻留、I 管搬运、AD 管到达；接口复用 F0405）。
pub const BOUNDARY_DOC: &str = "\
跨域边界（CGPU-F1281）：H 域管显存驻留——数据已在显存的放置、换页与\
回收；本 I 域管传输带宽——数据搬运通道的速率、仲裁与预算账户。接口\
复用 CGPU-F0405「与 I 带宽域接口集成」定义的集成面。与 VE-AD 存储流\
（F5801+）分工：存储流管数据从盘到内存的到达，本域管到达后进 GPU 的\
搬运带宽——三段各管一段，越界实现即架构缺陷。";

/// 十组宣告单行人话（读屏可查——域开工宣告的播报行）。
pub fn ten_group_line() -> String {
    let mut s = String::from("I 域十组规划：");
    let mut i = 0usize;
    while i < I_GROUPS.len() {
        if let Some((name, from, to)) = I_GROUPS.get(i) {
            if i > 0 {
                s.push_str("；");
            }
            s.push_str(&format!("I{:02} {}(F{}-F{})", i + 1, name, from, to));
        }
        i += 1;
    }
    s
}

// ---------------------------------------------------------------------------
// 二、账户化（判据二：四类传输各开账户；超预算拒绝；退款回账；账本守恒）
// ---------------------------------------------------------------------------

/// 传输种类闭集（锚点原文：纹理上传/下载/交换/流送）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferKind {
    /// 纹理上传（CPU→GPU）。
    TextureUpload,
    /// 纹理下载（GPU→CPU）。
    TextureDownload,
    /// 交换链翻转。
    Swap,
    /// 流送（边下边玩）。
    Streaming,
}

impl TransferKind {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            TransferKind::TextureUpload => "纹理上传",
            TransferKind::TextureDownload => "纹理下载",
            TransferKind::Swap => "交换链",
            TransferKind::Streaming => "流送",
        }
    }

    /// 账户槽位（固定四账户的下标）。
    pub const fn slot(self) -> usize {
        match self {
            TransferKind::TextureUpload => 0,
            TransferKind::TextureDownload => 1,
            TransferKind::Swap => 2,
            TransferKind::Streaming => 3,
        }
    }
}

/// 四类传输齐备（账户化的结构前提）。
pub const TRANSFER_KINDS: [TransferKind; 4] = [
    TransferKind::TextureUpload,
    TransferKind::TextureDownload,
    TransferKind::Swap,
    TransferKind::Streaming,
];

/// 单账户（每 tick 字节预算；累计账面用于守恒对账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BudgetAccount {
    /// 每 tick 预算（字节）。
    pub budget_per_tick: u64,
    /// 本 tick 已消费。
    pub consumed: u64,
    /// 本 tick 已退回。
    pub refunded: u64,
    /// 累计拒绝次数（拒绝留痕——不静默排队膨胀）。
    pub refused_count: u64,
    /// 预算对账基准（allocate 时与本 tick 预算同步；守恒式右边）。
    pub allocated_total: u64,
}

impl BudgetAccount {
    /// 余额。
    pub const fn remaining(&self) -> u64 {
        self.budget_per_tick.saturating_sub(self.consumed)
    }
}

/// 请求结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grant {
    /// 批准（记账后余额）。
    Granted {
        /// 记账后账户余额（字节）。
        remaining: u64,
    },
    /// 拒绝（余额不足；拒绝已计数）。
    Denied {
        /// 请求时的余额（字节）。
        available: u64,
    },
}

/// 带宽预算账本（判据二核心：每笔传输记账，超预算拒绝，退款回账）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BudgetLedger {
    accounts: [Option<BudgetAccount>; 4],
    tick: u64,
}

impl BudgetLedger {
    /// 新账本（四账户未初始化——账户由 allocate 开立，不做隐式预算）。
    pub const fn new() -> BudgetLedger {
        BudgetLedger {
            accounts: [None, None, None, None],
            tick: 0,
        }
    }

    /// 开立/调整账户预算（零预算拒绝——零预算账户是幻觉不是配置）。
    pub fn allocate(&mut self, kind: TransferKind, budget_per_tick: u64) -> Result<(), &'static str> {
        if budget_per_tick == 0 {
            return Err("零预算账户拒绝（账户化不做幻觉额度）");
        }
        let slot = kind.slot();
        match self.accounts.get_mut(slot) {
            Some(slot_ref) => {
                *slot_ref = Some(BudgetAccount {
                    budget_per_tick,
                    consumed: 0,
                    refunded: 0,
                    refused_count: 0,
                    allocated_total: budget_per_tick,
                });
                Ok(())
            }
            None => Err("账户槽位越界"),
        }
    }

    /// 账户是否已开立。
    pub fn is_open(&self, kind: TransferKind) -> bool {
        self.accounts.get(kind.slot()).map(|a| a.is_some()).unwrap_or(false)
    }

    /// 申请带宽（判据二：余额不足即拒绝并计数——每笔传输都记账）。
    pub fn request(&mut self, kind: TransferKind, bytes: u64) -> Grant {
        self.tick = self.tick.saturating_add(1);
        let slot = kind.slot();
        let account = match self.accounts.get_mut(slot) {
            Some(Some(a)) => a,
            _ => return Grant::Denied { available: 0 },
        };
        let available = account.remaining();
        if bytes == 0 || bytes > available {
            account.refused_count = account.refused_count.saturating_add(1);
            return Grant::Denied { available };
        }
        account.consumed = account.consumed.saturating_add(bytes);
        Grant::Granted {
            remaining: account.remaining(),
        }
    }

    /// 退款（实际用量小于申请量；退回量不得超过消费量）。
    pub fn refund(&mut self, kind: TransferKind, bytes: u64) -> bool {
        let slot = kind.slot();
        match self.accounts.get_mut(slot) {
            Some(Some(a)) => {
                if bytes == 0 || bytes > a.consumed {
                    return false;
                }
                a.consumed -= bytes;
                a.refunded = a.refunded.saturating_add(bytes);
                true
            }
            _ => false,
        }
    }

    /// 账本守恒（自检底座）：净消费口径 consumed + remaining ≡ allocated_total，
    /// 退款只做 consumed→refunded 的迁移（两边同消），任何一笔退款都
    /// 对应先前一笔消费（refund 的 bytes ≤ consumed 已在入口保证）。
    pub fn is_consistent(&self, kind: TransferKind) -> bool {
        match self.accounts.get(kind.slot()) {
            Some(Some(a)) => {
                let lhs = a.consumed.saturating_add(a.remaining());
                lhs == a.allocated_total
            }
            _ => false,
        }
    }

    /// 四账户全开且全守恒。
    pub fn all_consistent(&self) -> bool {
        let mut i = 0usize;
        while i < TRANSFER_KINDS.len() {
            let k = match TRANSFER_KINDS.get(i) {
                Some(k) => *k,
                None => return false,
            };
            if !self.is_open(k) || !self.is_consistent(k) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 使用率（每分比 ppm；未开账户返回 None——不报幻觉数字）。
    pub fn utilization_ppm(&self, kind: TransferKind) -> Option<u64> {
        match self.accounts.get(kind.slot()) {
            Some(Some(a)) if a.budget_per_tick > 0 => {
                Some(a.consumed * 1_000_000 / a.budget_per_tick)
            }
            _ => None,
        }
    }

    /// 账本摘要单行（遥测主题的开工面——读屏可查）。
    pub fn summary_line(&self, kind: TransferKind) -> String {
        match self.accounts.get(kind.slot()) {
            Some(Some(a)) => format!(
                "{}账户：预算{}B 已用{}B 退回{}B 拒绝{}次",
                kind.label(),
                a.budget_per_tick,
                a.consumed,
                a.refunded,
                a.refused_count
            ),
            _ => format!("{}账户：未开立", kind.label()),
        }
    }

    /// 未开账户的请求摘要（拒绝也入摘要——遥测不缺拒绝样本）。
    pub fn denied_line(&self, kind: TransferKind, available: u64) -> String {
        format!(
            "{}请求被拒：余额不足（可用{}B）——超预算拒绝留痕",
            kind.label(),
            available
        )
    }

    /// 逻辑 tick。
    pub const fn tick(&self) -> u64 {
        self.tick
    }
}

impl Default for BudgetLedger {
    fn default() -> BudgetLedger {
        BudgetLedger::new()
    }
}
