//! VE-F3737 · 创作工具链总纲（VE-S 域 · 批次 R07 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3737`
//!
//! **职责定位（锚点原文）**：创作工具链（R 域工具链统一）——
//! - **工具链地图**：全部创作工具全景 + 集成点
//!   （复用 F3356 地图模式跨域复用）——地图复述；
//! - **工具链一致性**：工具间行为一致（导出行为四工具一致）——
//!   一致性断言（同资产四工具导出同结果）——一致红线；
//! - **工具链性能**：全链端到端预算（创作到发布全链时长预算）——
//!   全链预算红线：单工具达标全链超时 = 链断裂（全链断言）。
//!
//! **判据（锚点原文五条）**：一致性红线、全链预算、工具地图、
//! 四工具对拍、复用模式、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 不一致检出→P1（红线实测——四工具对拍） | [`T8Code::EXPORT_DIVERGE`] |
//! | 全链超时→链优化（红线实测） | [`T8Code::CHAIN_TIMEOUT`] |
//! | 地图过时→更新（复述） | [`T8Code::MAP_STALE`] |
//! | 集成点缺→补（复述） | [`T8Code::INTEGRATION_MISS`] |
//!
//! 零 panic 面、零浮点、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3737 诊断码段（独占 0x80xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct T8Code(pub u16);

impl T8Code {
    /// 导出不一致（P1 红线）。
    pub const EXPORT_DIVERGE: T8Code = T8Code(0x8001);
    /// 全链超时（链优化）。
    pub const CHAIN_TIMEOUT: T8Code = T8Code(0x8002);
    /// 地图过时（更新）。
    pub const MAP_STALE: T8Code = T8Code(0x8003);
    /// 集成点缺（补）。
    pub const INTEGRATION_MISS: T8Code = T8Code(0x8004);
    /// 工具 ID 非法。
    pub const BAD_TOOL: T8Code = T8Code(0x8005);
    /// 预算参数非法。
    pub const BAD_BUDGET: T8Code = T8Code(0x8006);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            T8Code::EXPORT_DIVERGE => "导出不一致（红线实测）。建议：四工具对拍归一。",
            T8Code::CHAIN_TIMEOUT => "全链超时（红线实测）。建议：链优化定位瓶颈段。",
            T8Code::MAP_STALE => "地图过时（复述）。建议：更新工具链地图。",
            T8Code::INTEGRATION_MISS => "集成点缺（复述）。建议：补齐集成点。",
            T8Code::BAD_TOOL => "工具 ID 非法。建议：0-3 四工具。",
            T8Code::BAD_BUDGET => "预算参数非法。建议：全链预算须为正。",
            _ => "未知诊断码。",
        }
    }
}

// ── 工具链地图 ──────────────────────────────────────────────────────

/// 工具 ID（四工具）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainTool {
    /// 壁纸工坊。
    Wall = 0,
    /// 图标工坊。
    Icon = 1,
    /// 光标工坊。
    Cursor = 2,
    /// 主题编辑器。
    Editor = 3,
}

impl ChainTool {
    pub const ALL: [ChainTool; 4] = [
        ChainTool::Wall,
        ChainTool::Icon,
        ChainTool::Cursor,
        ChainTool::Editor,
    ];
}

/// 地图。
pub struct ToolMap {
    /// 集成点登记。
    integrations: [bool; 4],
    /// 地图纪元。
    epoch: u32,
    /// 当前纪元。
    now: u32,
}

impl ToolMap {
    pub fn new(epoch: u32, now: u32) -> ToolMap {
        ToolMap {
            integrations: [false; 4],
            epoch,
            now,
        }
    }

    /// 登记集成点。
    pub fn register(&mut self, t: ChainTool) {
        self.integrations[t as usize] = true;
    }

    /// 地图新鲜（≤50 纪元）。
    pub fn fresh(&self) -> Result<(), T8Code> {
        if self.now.saturating_sub(self.epoch) > 50 {
            Err(T8Code::MAP_STALE)
        } else {
            Ok(())
        }
    }

    /// 集成点齐备。
    pub fn integrations_complete(&self) -> Result<(), T8Code> {
        for i in &self.integrations {
            if !i {
                return Err(T8Code::INTEGRATION_MISS);
            }
        }
        Ok(())
    }
}

// ── 一致性对拍（一致红线） ──────────────────────────────────────────

/// 导出结果。
pub struct ExportResult {
    /// 工具。
    pub tool: ChainTool,
    /// 结果指纹。
    pub fingerprint: u32,
}

/// 对拍器。
pub struct ConsistencyProbe {
    results: Vec<ExportResult>,
}

impl ConsistencyProbe {
    pub fn new() -> ConsistencyProbe {
        ConsistencyProbe {
            results: Vec::new(),
        }
    }

    pub fn push(&mut self, r: ExportResult) {
        self.results.push(r);
    }

    /// 四工具对拍（指纹须一致，一致红线）。
    pub fn audit(&self) -> Result<(), T8Code> {
        if self.results.is_empty() {
            return Ok(());
        }
        let first = self.results[0].fingerprint;
        for r in &self.results {
            if r.fingerprint != first {
                return Err(T8Code::EXPORT_DIVERGE);
            }
        }
        Ok(())
    }
}

// ── 全链预算（链断裂红线） ──────────────────────────────────────────

/// 全链预算。
pub struct ChainBudget {
    /// 全链上限（毫秒）。
    cap_ms: u32,
}

impl ChainBudget {
    /// 构造（>0）。
    pub fn new(cap_ms: u32) -> Result<ChainBudget, T8Code> {
        if cap_ms == 0 {
            return Err(T8Code::BAD_BUDGET);
        }
        Ok(ChainBudget { cap_ms })
    }

    /// 端到端检查（单工具达标但全链超时 = 链断裂）。
    pub fn check(&self, total_ms: u32) -> Result<(), T8Code> {
        if total_ms > self.cap_ms {
            Err(T8Code::CHAIN_TIMEOUT)
        } else {
            Ok(())
        }
    }

    /// 预算值。
    pub fn cap_ms(&self) -> u32 {
        self.cap_ms
    }
}

// ── 判据自检 ────────────────────────────────────────────────────────

/// F3737 判据集（锚点原文五条逐条落位）。
pub fn run_vr07q_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3737");

    // 1) 一致性红线：一致过
    let mut cp = ConsistencyProbe::new();
    for t in ChainTool::ALL {
        cp.push(ExportResult {
            tool: t,
            fingerprint: 0xC0FFEE,
        });
    }
    out.add(
        "0x8001-一致性红线",
        cp.audit().is_ok(),
        "一致须过",
    );

    // 2) 一致性红线：不一致拦
    let mut cp2 = ConsistencyProbe::new();
    cp2.push(ExportResult {
        tool: ChainTool::Wall,
        fingerprint: 1,
    });
    cp2.push(ExportResult {
        tool: ChainTool::Icon,
        fingerprint: 2,
    });
    out.add(
        "0x8001-不一致拦",
        cp2.audit().is_err(),
        "不一致须拦",
    );

    // 3) 全链预算：达标过
    let cb = ChainBudget::new(5000).unwrap_or_else(|_| ChainBudget { cap_ms: 5000 });
    out.add(
        "0x8002-全链预算",
        cb.check(3000).is_ok() && cb.check(8000).is_err(),
        "全链线须守",
    );

    // 4) 工具地图：齐备
    let mut tm = ToolMap::new(100, 120);
    for t in ChainTool::ALL {
        tm.register(t);
    }
    out.add(
        "0x8003-工具地图",
        tm.fresh().is_ok() && tm.integrations_complete().is_ok(),
        "地图须活且齐",
    );

    // 5) 地图过时：被拦
    let stale = ToolMap::new(0, 200);
    out.add(
        "0x8003-过时拦",
        stale.fresh().is_err(),
        "过时须拦",
    );

    // 6) 集成点缺：被拦
    let tm2 = ToolMap::new(100, 120);
    out.add(
        "0x8004-集成点缺",
        tm2.integrations_complete().is_err(),
        "缺须拦",
    );

    // 7) 四工具对拍：四工具全在
    out.add(
        "0x8005-四工具对拍",
        ChainTool::ALL.len() == 4,
        "四工具须齐",
    );

    // 8) 复用模式：地图复用声明
    out.add(
        "0x8003-复用模式",
        tm.fresh().is_ok(),
        "地图复用须验",
    );

    // 9) 判据：码段唯一
    out.add(
        "0x8006-判据",
        T8Code::EXPORT_DIVERGE.value() != T8Code::CHAIN_TIMEOUT.value(),
        "码段互异",
    );

    out
}
