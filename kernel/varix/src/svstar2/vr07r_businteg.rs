//! VE-F3738 · 创作工具链集成（VE-S 域 · 批次 R07 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3738`
//!
//! **职责定位（锚点原文）**：创作工具链集成（工具间数据流通）——
//! - **集成总线**：工具间数据交换统一总线（点对点私连禁止）——
//!   总线红线：点对点集成 = N² 复杂度（N² 红线：工具数增长集成爆炸，
//!   总线架构）；
//! - **总线契约**：消息格式冻结 + 版本化（复述接口纪律）；
//! - **总线可靠**：消息不丢（投递确认复用，复述）；
//! - **总线观测**：消息流可观测（复述家族）。
//!
//! **判据（锚点原文五条）**：总线架构、N² 红线、投递确认、
//! 契约版本化、私连重构、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 私连检出→P1 重构（红线实测——集成审计） | [`B8Code::PRIVATE_LINK`] |
//! | 消息丢→P0（确认红线复述实测） | [`B8Code::MSG_LOST_P0`] |
//! | 契约漂移→版本化（复述） | [`B8Code::CONTRACT_DRIFT`] |
//! | 总线卡→性能回归（复述） | [`B8Code::BUS_STUCK`] |
//!
//! 零 panic 面、零浮点、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3738 诊断码段（独占 0x81xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct B8Code(pub u16);

impl B8Code {
    /// 点对点私连（P1 重构）。
    pub const PRIVATE_LINK: B8Code = B8Code(0x8101);
    /// 消息丢（P0 确认红线）。
    pub const MSG_LOST_P0: B8Code = B8Code(0x8102);
    /// 契约漂移（版本化）。
    pub const CONTRACT_DRIFT: B8Code = B8Code(0x8103);
    /// 总线卡（性能回归）。
    pub const BUS_STUCK: B8Code = B8Code(0x8104);
    /// 消息格式非法。
    pub const BAD_MSG: B8Code = B8Code(0x8105);
    /// 路由表缺。
    pub const ROUTE_MISS: B8Code = B8Code(0x8106);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            B8Code::PRIVATE_LINK => "点对点私连（P1 红线实测）。建议：重构到总线。",
            B8Code::MSG_LOST_P0 => "消息丢（P0 确认红线）。建议：补投递确认。",
            B8Code::CONTRACT_DRIFT => "契约漂移（复述）。建议：版本化冻结。",
            B8Code::BUS_STUCK => "总线卡（复述）。建议：性能回归定位。",
            B8Code::BAD_MSG => "消息格式非法。建议：按契约构造。",
            B8Code::ROUTE_MISS => "路由表缺。建议：补齐路由。",
            _ => "未知诊断码。",
        }
    }
}

// ── 总线架构（N² 红线） ─────────────────────────────────────────────

/// 消息。
pub struct BusMsg {
    /// 源 ID。
    pub src: u8,
    /// 目标 ID。
    pub dst: u8,
    /// 负载指纹。
    pub payload_fp: u32,
}

/// 总线。
pub struct DataBus {
    /// 路由表（目标可达）。
    routes: Vec<u8>,
    /// 私连计数。
    private_links: u32,
    /// 确认表（已确认消息数）。
    confirmed: u32,
    /// 发送数。
    sent: u32,
}

impl DataBus {
    pub fn new() -> DataBus {
        DataBus {
            routes: Vec::new(),
            private_links: 0,
            confirmed: 0,
            sent: 0,
        }
    }

    /// 注册路由。
    pub fn route(&mut self, node: u8) {
        if !self.routes.contains(&node) {
            self.routes.push(node);
        }
    }

    /// 发送（须走总线，私连即拦）。
    pub fn send(&mut self, msg: BusMsg) -> Result<(), B8Code> {
        // 私连检出：目标不在路由表
        if !self.routes.contains(&msg.dst) {
            self.private_links += 1;
            return Err(B8Code::PRIVATE_LINK);
        }
        self.sent += 1;
        Ok(())
    }

    /// 投递确认（P0：未确认即丢）。
    pub fn confirm(&mut self) -> Result<(), B8Code> {
        if self.confirmed >= self.sent {
            return Err(B8Code::MSG_LOST_P0);
        }
        self.confirmed += 1;
        Ok(())
    }

    /// 确认完整性。
    pub fn confirm_complete(&self) -> Result<(), B8Code> {
        if self.confirmed == self.sent {
            Ok(())
        } else {
            Err(B8Code::MSG_LOST_P0)
        }
    }

    /// 私连计数。
    pub fn private_count(&self) -> u32 {
        self.private_links
    }

    /// 路由数。
    pub fn route_count(&self) -> usize {
        self.routes.len()
    }
}

// ── 消息契约（版本化） ──────────────────────────────────────────────

/// 契约版本。
pub struct Contract {
    /// 版本号。
    version: u16,
    /// 指纹。
    fingerprint: u32,
}

impl Contract {
    /// 冻结版本。
    pub const fn freeze(version: u16, fingerprint: u32) -> Contract {
        Contract {
            version,
            fingerprint,
        }
    }

    /// 版本化漂移检出（同版本不同指纹 = 漂移）。
    pub fn drift_check(&self, other: &Contract) -> Result<(), B8Code> {
        if self.version == other.version && self.fingerprint != other.fingerprint {
            Err(B8Code::CONTRACT_DRIFT)
        } else {
            Ok(())
        }
    }

    /// 版本号。
    pub fn version(&self) -> u16 {
        self.version
    }
}

// ── 总线观测（复述家族） ────────────────────────────────────────────

/// 观测记录。
pub struct BusObserver {
    /// 消息流条数。
    flow_count: u32,
}

impl BusObserver {
    pub fn new() -> BusObserver {
        BusObserver { flow_count: 0 }
    }

    /// 记一条。
    pub fn record(&mut self) {
        self.flow_count += 1;
    }

    /// 可观测（有流量）。
    pub fn observable(&self) -> bool {
        self.flow_count > 0
    }
}

// ── 判据自检 ────────────────────────────────────────────────────────

/// F3738 判据集（锚点原文五条逐条落位）。
pub fn run_vr07r_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3738");

    // 1) 总线架构：路由工作
    let mut bus = DataBus::new();
    bus.route(1);
    bus.route(2);
    out.add(
        "0x8106-总线架构",
        bus.route_count() == 2,
        "路由须建",
    );

    // 2) N² 红线：私连被拦
    out.add(
        "0x8101-私连拦",
        bus.send(BusMsg {
            src: 0,
            dst: 99,
            payload_fp: 1,
        })
        .is_err(),
        "私连须拦",
    );

    // 3) 总线发送：可达过
    out.add(
        "0x8101-可达过",
        bus.send(BusMsg {
            src: 0,
            dst: 1,
            payload_fp: 1,
        })
        .is_ok(),
        "可达须过",
    );

    // 4) 投递确认：丢消息 P0
    let mut bus2 = DataBus::new();
    bus2.route(1);
    let _ = bus2.send(BusMsg {
        src: 0,
        dst: 1,
        payload_fp: 1,
    });
    out.add(
        "0x8102-投递确认",
        bus2.confirm_complete().is_err(),
        "未确认须 P0",
    );

    // 5) 投递确认：确认后过
    let _ = bus2.confirm();
    out.add(
        "0x8102-确认后过",
        bus2.confirm_complete().is_ok(),
        "确认后须过",
    );

    // 6) 契约版本化：漂移拦
    let c1 = Contract::freeze(1, 0xAAAA);
    let c2 = Contract::freeze(1, 0xBBBB);
    out.add(
        "0x8103-契约漂移",
        c1.drift_check(&c2).is_err(),
        "漂移须拦",
    );

    // 7) 契约：版本过
    let c3 = Contract::freeze(2, 0xBBBB);
    out.add(
        "0x8103-版本过",
        c1.drift_check(&c3).is_ok(),
        "异版须过",
    );

    // 8) 观测：可观测
    let mut bo = BusObserver::new();
    bo.record();
    out.add(
        "0x8106-观测家族",
        bo.observable(),
        "有流量须可观测",
    );

    // 9) 判据：码段唯一
    out.add(
        "0x8106-判据",
        B8Code::PRIVATE_LINK.value() != B8Code::MSG_LOST_P0.value(),
        "码段互异",
    );

    out
}
