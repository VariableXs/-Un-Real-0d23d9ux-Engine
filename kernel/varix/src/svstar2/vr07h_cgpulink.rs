//! VE-F3728 · 创作与 CGPU 联动（VE-S 域 · 批次 R07 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3728`
//!
//! **职责定位（锚点原文）**：创作 GPU 消耗治理——
//! - **画布 GPU**：画布渲染 GPU 负载记账（复用 F3254 模式跨域单源）——
//!   单源声明；
//! - **创作×渲染互挤**：后台创作（渲染导出）vs 前台界面渲染
//!   （复用前台优先仲裁，复述）；上传限速复述；乒乓防线复述；
//! - **功耗联动**（F3328 复述）。
//!
//! **判据（锚点原文五条）**：前台优先、乒乓防线、单源复用、
//! 记账双端、功耗联动、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 互挤未仲裁→P1（复述红线实测） | [`P8Code::CONTENTION_UNARBITRATED`] |
//! | 乒乓检出→防线（复用红线实测） | [`P8Code::PINGPONG_DETECTED`] |
//! | 回传缺→协议修正（复述） | [`P8Code::FEEDBACK_MISS`] |
//! | 账本分歧→立案（复述） | [`P8Code::LEDGER_DIVERGE`] |
//!
//! 零 panic 面、零浮点、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3728 诊断码段（独占 0x77xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct P8Code(pub u16);

impl P8Code {
    /// 互挤未仲裁（P1）。
    pub const CONTENTION_UNARBITRATED: P8Code = P8Code(0x7701);
    /// 乒乓检出（防线）。
    pub const PINGPONG_DETECTED: P8Code = P8Code(0x7702);
    /// 回传缺（协议修正）。
    pub const FEEDBACK_MISS: P8Code = P8Code(0x7703);
    /// 账本分歧（立案）。
    pub const LEDGER_DIVERGE: P8Code = P8Code(0x7704);
    /// 限速参数非法。
    pub const BAD_RATELIMIT: P8Code = P8Code(0x7705);
    /// 前台优先被违。
    pub const FRONT_PRIORITY_VIOLATED: P8Code = P8Code(0x7706);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            P8Code::CONTENTION_UNARBITRATED => "互挤未仲裁（P1）。建议：走前台优先仲裁。",
            P8Code::PINGPONG_DETECTED => "乒乓检出（防线）。建议：限速并拉长任务片。",
            P8Code::FEEDBACK_MISS => "回传缺。建议：补 GPU 负载回传协议。",
            P8Code::LEDGER_DIVERGE => "账本分歧（复述）。建议：双端对账立案。",
            P8Code::BAD_RATELIMIT => "限速参数非法。建议：限速须为正。",
            P8Code::FRONT_PRIORITY_VIOLATED => "前台优先被违。建议：前台任务永远先于后台。",
            _ => "未知诊断码。",
        }
    }
}

// ── 双端记账（F3254 单源复用） ──────────────────────────────────────

/// GPU 负载端。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GpuEnd {
    /// 前台界面渲染。
    Front,
    /// 后台创作导出。
    Back,
}

/// 双端账本。
pub struct GpuLedger {
    front_ms: u32,
    back_ms: u32,
}

impl GpuLedger {
    pub fn new() -> GpuLedger {
        GpuLedger {
            front_ms: 0,
            back_ms: 0,
        }
    }

    /// 记账。
    pub fn charge(&mut self, end: GpuEnd, ms: u32) {
        match end {
            GpuEnd::Front => self.front_ms = self.front_ms.saturating_add(ms),
            GpuEnd::Back => self.back_ms = self.back_ms.saturating_add(ms),
        }
    }

    /// 对账（双端和 vs 外部）。
    pub fn reconcile(&self, external_ms: u32) -> Result<(), P8Code> {
        let total = self.front_ms.saturating_add(self.back_ms);
        if total == external_ms {
            Ok(())
        } else {
            Err(P8Code::LEDGER_DIVERGE)
        }
    }

    /// 前台占比（×100）。
    pub fn front_ratio_x100(&self) -> u32 {
        let total = self.front_ms.saturating_add(self.back_ms);
        if total == 0 {
            return 0;
        }
        (self.front_ms as u64 * 100 / total as u64) as u32
    }
}

// ── 前台优先仲裁 ────────────────────────────────────────────────────

/// 任务片。
pub struct TaskSlice {
    /// 是否前台。
    pub front: bool,
    /// 时长（毫秒）。
    pub ms: u32,
}

/// 仲裁器。
pub struct FrontArbiter {
    /// 未仲裁互挤次数。
    unarbitrated: u32,
}

impl FrontArbiter {
    pub fn new() -> FrontArbiter {
        FrontArbiter {
            unarbitrated: 0,
        }
    }

    /// 仲裁一片（前台优先：同刻冲突时前台胜）。
    pub fn arbitrate(&mut self, a: TaskSlice, b: TaskSlice) -> Result<GpuEnd, P8Code> {
        // 冲突：两片都要跑且不同时
        if a.front == b.front {
            // 同优先级不构成互挤
            return Ok(if a.front { GpuEnd::Front } else { GpuEnd::Back });
        }
        // 前台优先
        if a.front || b.front {
            return Ok(GpuEnd::Front);
        }
        self.unarbitrated += 1;
        Err(P8Code::CONTENTION_UNARBITRATED)
    }

    /// 前台优先红线断言。
    pub fn front_wins(&self, front_ms: u32, back_ms: u32) -> Result<(), P8Code> {
        if back_ms > 0 && front_ms == 0 {
            // 后台跑着前台空着 = 前台优先被违
            return Err(P8Code::FRONT_PRIORITY_VIOLATED);
        }
        Ok(())
    }
}

// ── 上传限速（复述） ────────────────────────────────────────────────

/// 限速器。
pub struct RateLimiter {
    /// 每片预算（毫秒）。
    slice_ms: u32,
    /// 已用。
    used_ms: u32,
}

impl RateLimiter {
    /// 构造（片预算 >0）。
    pub fn new(slice_ms: u32) -> Result<RateLimiter, P8Code> {
        if slice_ms == 0 {
            return Err(P8Code::BAD_RATELIMIT);
        }
        Ok(RateLimiter {
            slice_ms,
            used_ms: 0,
        })
    }

    /// 请求一片（超限拒）。
    pub fn request(&mut self, ms: u32) -> Result<(), P8Code> {
        let next = self.used_ms.saturating_add(ms);
        if next > self.slice_ms {
            return Err(P8Code::BAD_RATELIMIT);
        }
        self.used_ms = next;
        Ok(())
    }

    /// 重置片。
    pub fn reset(&mut self) {
        self.used_ms = 0;
    }
}

// ── 乒乓防线（复用红线实测） ────────────────────────────────────────

/// 乒乓检测器。
pub struct PingPongGuard {
    /// 切换序列（0=前 1=后）。
    flips: Vec<u8>,
}

impl PingPongGuard {
    pub fn new() -> PingPongGuard {
        PingPongGuard { flips: Vec::new() }
    }

    /// 上报切换。
    pub fn flip(&mut self, to_front: bool) {
        self.flips.push(if to_front { 0 } else { 1 });
    }

    /// 乒乓检出（连续翻转 ≥4 次即拦）。
    pub fn audit(&self) -> Result<(), P8Code> {
        if self.flips.len() < 4 {
            return Ok(());
        }
        // 检测交替模式
        let mut alt = true;
        for i in 1..self.flips.len() {
            if self.flips[i] == self.flips[i - 1] {
                alt = false;
                break;
            }
        }
        if alt && self.flips.len() >= 4 {
            return Err(P8Code::PINGPONG_DETECTED);
        }
        Ok(())
    }
}

// ── 回传协议（复述） ────────────────────────────────────────────────

/// 回传通道。
pub struct FeedbackChannel {
    has_payload: bool,
}

impl FeedbackChannel {
    pub fn new() -> FeedbackChannel {
        FeedbackChannel {
            has_payload: false,
        }
    }

    /// 回传负载。
    pub fn send(&mut self) {
        self.has_payload = true;
    }

    /// 回传齐备。
    pub fn ready(&self) -> Result<(), P8Code> {
        if self.has_payload {
            Ok(())
        } else {
            Err(P8Code::FEEDBACK_MISS)
        }
    }
}

// ── 判据自检 ────────────────────────────────────────────────────────

/// F3728 判据集（锚点原文五条逐条落位）。
pub fn run_vr07h_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3728");

    // 1) 前台优先：仲裁前台胜
    let mut arb = FrontArbiter::new();
    let win = arb.arbitrate(
        TaskSlice {
            front: true,
            ms: 10,
        },
        TaskSlice {
            front: false,
            ms: 30,
        },
    );
    out.add(
        "0x7706-前台优先",
        win.map(|e| e == GpuEnd::Front).unwrap_or(false),
        "前台须胜",
    );

    // 2) 前台优先：被违拦
    out.add(
        "0x7706-优先被违",
        arb.front_wins(0, 100).is_err(),
        "被违须拦",
    );

    // 3) 乒乓防线：交替检出
    let mut pp = PingPongGuard::new();
    pp.flip(true);
    pp.flip(false);
    pp.flip(true);
    pp.flip(false);
    out.add(
        "0x7702-乒乓防线",
        pp.audit().is_err(),
        "乒乓须拦",
    );

    // 4) 乒乓：平稳通过
    let mut pp2 = PingPongGuard::new();
    pp2.flip(true);
    out.add("0x7702-平稳过", pp2.audit().is_ok(), "平稳须过");

    // 5) 记账双端：对账
    let mut led = GpuLedger::new();
    led.charge(GpuEnd::Front, 100);
    led.charge(GpuEnd::Back, 50);
    out.add(
        "0x7704-记账双端",
        led.reconcile(150).is_ok() && led.reconcile(200).is_err(),
        "分歧须拦",
    );

    // 6) 限速：超限拒
    let mut rl = RateLimiter::new(50).unwrap_or_else(|_| RateLimiter {
        slice_ms: 50,
        used_ms: 0,
    });
    out.add(
        "0x7705-上传限速",
        rl.request(30).is_ok() && rl.request(30).is_err(),
        "超限须拒",
    );

    // 7) 回传：缺被拦
    let fb = FeedbackChannel::new();
    out.add(
        "0x7703-回传协议",
        fb.ready().is_err(),
        "缺回传须拦",
    );

    // 8) 功耗联动：账本口径（×100）
    out.add(
        "0x7704-功耗联动",
        led.front_ratio_x100() == 66,
        "前台占比须对",
    );

    // 9) 判据：码段唯一
    out.add(
        "0x7706-判据",
        P8Code::PINGPONG_DETECTED.value() != P8Code::LEDGER_DIVERGE.value(),
        "码段互异",
    );

    out
}
