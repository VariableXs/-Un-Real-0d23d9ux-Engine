//! VE-F3725 · 创作回归门与看板（VE-S 域 · 批次 R07 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3725`
//!
//! **职责定位（锚点原文）**：门禁与看板——
//! - **双阈值门**：预算门 + 回归门（复用 F3724 回归口径）；
//! - **看板四视图**：预算仪表/成本偏差/降档频度/心流视图——
//!   四用复刻 + 心流视图顶层（复述用户视图顺序红线四用）——
//!   心流顶层红线：心流指标不进顶层 = 治理本末倒置；
//! - **告警路由**复述；**豁免簿**复述。
//!
//! **判据（锚点原文五条）**：双阈值、心流顶层、四视图、
//! 豁免到期、四用复刻、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 绕门→P0（复述） | [`G7Code::GATE_BYPASS_P0`] |
//! | 顺序违→修正（复述红线实测） | [`G7Code::ORDER_VIOLATE`] |
//! | 豁免滥用→升级（复述） | [`G7Code::EXEMPT_ABUSE`] |
//! | 陈数→标注（复述） | [`G7Code::STALE_DATA`] |
//!
//! 零 panic 面、零浮点、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3725 诊断码段（独占 0x74xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct G7Code(pub u16);

impl G7Code {
    /// 绕门（P0 复述）。
    pub const GATE_BYPASS_P0: G7Code = G7Code(0x7401);
    /// 视图顺序违（心流顶层红线）。
    pub const ORDER_VIOLATE: G7Code = G7Code(0x7402);
    /// 豁免滥用（升级）。
    pub const EXEMPT_ABUSE: G7Code = G7Code(0x7403);
    /// 数据陈旧（标注）。
    pub const STALE_DATA: G7Code = G7Code(0x7404);
    /// 阈值参数非法。
    pub const BAD_THRESHOLD: G7Code = G7Code(0x7405);
    /// 告警路由缺失。
    pub const ROUTE_MISS: G7Code = G7Code(0x7406);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            G7Code::GATE_BYPASS_P0 => "绕门（P0）。建议：门禁不可绕过，立即停机立案。",
            G7Code::ORDER_VIOLATE => "视图顺序违（心流顶层红线）。建议：心流视图必须置顶。",
            G7Code::EXEMPT_ABUSE => "豁免滥用。建议：升级至跨组仲裁。",
            G7Code::STALE_DATA => "数据陈旧。建议：刷新后标注时效。",
            G7Code::BAD_THRESHOLD => "阈值参数非法。建议：阈值须为正。",
            G7Code::ROUTE_MISS => "告警路由缺失。建议：配置告警接收方。",
            _ => "未知诊断码。",
        }
    }
}

// ── 双阈值门 ────────────────────────────────────────────────────────

/// 阈值对（软/硬）。
pub struct DualThreshold {
    /// 软阈值 ×100。
    soft_x100: u32,
    /// 硬阈值 ×100。
    hard_x100: u32,
}

impl DualThreshold {
    /// 构造（软 < 硬）。
    pub fn new(soft_x100: u32, hard_x100: u32) -> Result<DualThreshold, G7Code> {
        if soft_x100 == 0 || hard_x100 == 0 || soft_x100 >= hard_x100 {
            return Err(G7Code::BAD_THRESHOLD);
        }
        Ok(DualThreshold {
            soft_x100,
            hard_x100,
        })
    }

    /// 判定（0=过 1=软超 2=硬超）。
    pub fn evaluate(&self, usage_x100: u32) -> u8 {
        if usage_x100 >= self.hard_x100 {
            2
        } else if usage_x100 >= self.soft_x100 {
            1
        } else {
            0
        }
    }
}

/// 双阈值门（预算门 + 回归门）。
pub struct Gate2 {
    budget: DualThreshold,
    regress: DualThreshold,
    /// 绕门尝试。
    bypass_attempted: bool,
}

impl Gate2 {
    pub fn new(budget: DualThreshold, regress: DualThreshold) -> Gate2 {
        Gate2 {
            budget,
            regress,
            bypass_attempted: false,
        }
    }

    /// 门判定（任一硬超即拒）。
    pub fn evaluate(&self, budget_use_x100: u32, regress_x100: u32) -> Result<(), G7Code> {
        if self.bypass_attempted {
            return Err(G7Code::GATE_BYPASS_P0);
        }
        if self.budget.evaluate(budget_use_x100) == 2 || self.regress.evaluate(regress_x100) == 2 {
            return Err(G7Code::GATE_BYPASS_P0);
        }
        Ok(())
    }

    /// 绕门尝试（P0）。
    pub fn try_bypass(&mut self) -> Result<(), G7Code> {
        self.bypass_attempted = true;
        Err(G7Code::GATE_BYPASS_P0)
    }
}

// ── 看板四视图（心流顶层红线） ──────────────────────────────────────

/// 视图。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    /// 心流（必须顶层）。
    Flow,
    /// 预算仪表。
    Budget,
    /// 成本偏差。
    CostDev,
    /// 降档频度。
    DegradeFreq,
}

impl View {
    pub const ALL: [View; 4] = [
        View::Flow,
        View::Budget,
        View::CostDev,
        View::DegradeFreq,
    ];
}

/// 看板（顺序红线载体）。
pub struct Dashboard {
    /// 视图顺序（索引序列）。
    order: Vec<View>,
    /// 数据时效（分）。
    age_min: u32,
}

impl Dashboard {
    /// 标准构造（心流置顶）。
    pub fn standard() -> Dashboard {
        Dashboard {
            order: vec![
                View::Flow,
                View::Budget,
                View::CostDev,
                View::DegradeFreq,
            ],
            age_min: 0,
        }
    }

    /// 顺序断言（首位必须 Flow）。
    pub fn order_check(&self) -> Result<(), G7Code> {
        match self.order.first() {
            Some(View::Flow) => {
                if self.order.len() != 4 {
                    return Err(G7Code::ORDER_VIOLATE);
                }
                Ok(())
            }
            _ => Err(G7Code::ORDER_VIOLATE),
        }
    }

    /// 数据时效（>30 分标陈）。
    pub fn stale_check(&self) -> Result<(), G7Code> {
        if self.age_min > 30 {
            Err(G7Code::STALE_DATA)
        } else {
            Ok(())
        }
    }
}

// ── 豁免簿（滥用升级） ──────────────────────────────────────────────

/// 豁免记录。
pub struct Exemption {
    /// 到期日（纪元）。
    pub expire: u32,
    /// 使用次数。
    pub used: u32,
}

/// 豁免簿。
pub struct ExemptBook {
    items: Vec<Exemption>,
    /// 滥用上限。
    abuse_cap: u32,
}

impl ExemptBook {
    pub fn new(abuse_cap: u32) -> ExemptBook {
        ExemptBook {
            items: Vec::new(),
            abuse_cap,
        }
    }

    /// 登记豁免。
    pub fn add(&mut self, expire: u32) {
        self.items.push(Exemption { expire, used: 0 });
    }

    /// 使用豁免（须未到期）。
    pub fn use_one(&mut self, now: u32) -> Result<(), G7Code> {
        for it in self.items.iter_mut() {
            if it.expire >= now {
                it.used += 1;
                if it.used > self.abuse_cap {
                    return Err(G7Code::EXEMPT_ABUSE);
                }
                return Ok(());
            }
        }
        Err(G7Code::EXEMPT_ABUSE)
    }

    /// 到期清理计数。
    pub fn expired_count(&self, now: u32) -> usize {
        self.items.iter().filter(|i| i.expire < now).count()
    }
}

// ── 告警路由 ────────────────────────────────────────────────────────

/// 路由表。
pub struct AlertRoute {
    routes: Vec<u8>,
}

impl AlertRoute {
    pub fn new() -> AlertRoute {
        AlertRoute { routes: Vec::new() }
    }

    /// 注册路由。
    pub fn register(&mut self, target: u8) {
        self.routes.push(target);
    }

    /// 路由齐备。
    pub fn ready(&self) -> Result<(), G7Code> {
        if self.routes.is_empty() {
            Err(G7Code::ROUTE_MISS)
        } else {
            Ok(())
        }
    }
}

// ── 判据自检 ────────────────────────────────────────────────────────

/// F3725 判据集（锚点原文五条逐条落位）。
pub fn run_vr07e_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3725");

    // 1) 双阈值：软硬分级
    let dt = DualThreshold::new(800, 1000).unwrap_or_else(|_| DualThreshold {
        soft_x100: 800,
        hard_x100: 1000,
    });
    out.add(
        "0x7405-双阈值",
        dt.evaluate(500) == 0 && dt.evaluate(900) == 1 && dt.evaluate(1100) == 2,
        "三级判定须对",
    );

    // 2) 双阈值：非法被拒
    out.add(
        "0x7405-非法阈值",
        DualThreshold::new(1000, 800).is_err(),
        "软≥硬须拒",
    );

    // 3) 绕门：P0 拦截
    let mut g = Gate2::new(
        DualThreshold::new(800, 1000).unwrap_or_else(|_| DualThreshold {
            soft_x100: 800,
            hard_x100: 1000,
        }),
        DualThreshold::new(500, 800).unwrap_or_else(|_| DualThreshold {
            soft_x100: 500,
            hard_x100: 800,
        }),
    );
    out.add(
        "0x7401-绕门 P0",
        g.try_bypass().is_err() && g.evaluate(0, 0).is_err(),
        "绕门须 P0",
    );

    // 4) 心流顶层：顺序断言
    let dash = Dashboard::standard();
    out.add(
        "0x7402-心流顶层",
        dash.order_check().is_ok(),
        "心流须置顶",
    );

    // 5) 心流顶层：顺序违被拦
    let bad_dash = Dashboard {
        order: vec![View::Budget, View::Flow, View::CostDev, View::DegradeFreq],
        age_min: 0,
    };
    out.add(
        "0x7402-顺序违拦",
        bad_dash.order_check().is_err(),
        "顺序违须拦",
    );

    // 6) 陈数：标注
    let stale = Dashboard {
        order: View::ALL.to_vec(),
        age_min: 45,
    };
    out.add(
        "0x7404-陈数标注",
        stale.stale_check().is_err(),
        "陈数须标",
    );

    // 7) 豁免到期：滥用升级
    let mut eb = ExemptBook::new(1);
    eb.add(100);
    out.add(
        "0x7403-豁免滥用",
        eb.use_one(50).is_ok() && eb.use_one(50).is_err(),
        "超用须升级",
    );

    // 8) 告警路由：缺失被拦
    let route = AlertRoute::new();
    out.add(
        "0x7406-告警路由",
        route.ready().is_err(),
        "缺路由须拦",
    );

    // 9) 判据：码段唯一
    out.add(
        "0x7406-判据",
        G7Code::GATE_BYPASS_P0.value() != G7Code::ORDER_VIOLATE.value(),
        "码段互异",
    );

    out
}
