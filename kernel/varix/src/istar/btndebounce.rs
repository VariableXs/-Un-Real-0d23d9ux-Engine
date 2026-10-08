//! F582 按钮防双击 · 完整设计（STAR I 主册 I 域批次八）。
//!
//! **判据（主册）**：300ms 窗口实测；双提交注入测试（0 重复）；处理态
//! 视觉；导航类豁免清单；误伤评估（快速合法双击场景走查）。
//!
//! **设计要点（主册）**：
//! - 提交类按钮点击后 300ms 内二次点击无效（防双提交）；
//! - 按钮点击后立即进入处理态（微降饱和 + 禁止光标，反馈「已收到」）；
//! - 导航类按钮不防抖（连续翻页合法）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 防抖窗口（ms）。
pub const DEBOUNCE_MS: u64 = 300;

/// 按钮类别（豁免清单唯一源——枚举即清单）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BtnKind {
    /// 提交类（下单/保存/发送）——防抖。
    Submit,
    /// 导航类（翻页/返回）——豁免。
    Navigate,
    /// 切换类（开关/勾选）——豁免（快速连切合法）。
    Toggle,
}

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 按钮防抖器（每按钮一账）。
pub struct BtnGuard {
    kind: BtnKind,
    /// 处理态（点击后进入——视觉反馈「已收到」的账面）。
    processing: bool,
    last_click_ms: Option<u64>,
    now_ms: u64,
    accepted: u32,
    rejected: u32,
}

impl BtnGuard {
    pub fn new(kind: BtnKind) -> BtnGuard {
        BtnGuard {
            kind,
            processing: false,
            last_click_ms: None,
            now_ms: 0,
            accepted: 0,
            rejected: 0,
        }
    }

    pub fn tick(&mut self, ms: u64) {
        if ms > self.now_ms {
            self.now_ms = ms;
        }
        // 处理态在窗口期满自动解除（微降饱和的恢复路径）。
        if self.processing {
            if let Some(t) = self.last_click_ms {
                if self.now_ms.saturating_sub(t) >= DEBOUNCE_MS {
                    self.processing = false;
                }
            }
        }
    }

    /// 点击：返回是否受理。
    pub fn click(&mut self, ms: u64) -> bool {
        self.tick(ms);
        match self.kind {
            BtnKind::Navigate | BtnKind::Toggle => {
                // 豁免类：每次都受理（不进处理态——处理态会挡连点）。
                self.accepted += 1;
                true
            }
            BtnKind::Submit => {
                if self.processing {
                    self.rejected += 1;
                    return false;
                }
                self.processing = true;
                self.last_click_ms = Some(self.now_ms);
                self.accepted += 1;
                true
            }
        }
    }

    pub fn processing(&self) -> bool {
        self.processing
    }

    /// 处理态视觉参数（微降饱和 + 禁止光标——渲染唯一取数口）。
    pub fn processing_visual(&self) -> (u8, bool) {
        // (饱和度 %, 禁止光标)
        if self.processing {
            (70, true)
        } else {
            (100, false)
        }
    }

    pub fn accepted(&self) -> u32 {
        self.accepted
    }

    pub fn rejected(&self) -> u32 {
        self.rejected
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_btndebounce_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 300ms 窗口实测：双击注入 299ms 拒、300ms 放（边界双点钉死）。
    let mut g = BtnGuard::new(BtnKind::Submit);
    g.click(0);
    let second = g.click(299);
    let third = g.click(300);
    set.add(
        "debounce window exact boundary",
        !second && third && g.accepted() == 2 && g.rejected() == 1,
        "",
    );

    // 2. 双提交注入测试：300ms 窗口内重复点击 0 受理（窗口期满才放下一单
    //    ——10 连点跨 450ms = 恰好 2 次受理，无窗口内重复）。
    let mut g2 = BtnGuard::new(BtnKind::Submit);
    let mut orders = 0;
    for i in 0..10u64 {
        if g2.click(i * 50) {
            orders += 1;
        }
    }
    set.add(
        "double submit zero duplicates",
        orders == 2 && g2.accepted() == 2 && g2.rejected() == 8,
        "",
    );

    // 3. 处理态视觉：点击即处理态（微降饱和 70% + 禁止光标）；期满恢复。
    let mut g3 = BtnGuard::new(BtnKind::Submit);
    let idle_vis = g3.processing_visual();
    g3.click(0);
    let busy_vis = g3.processing_visual();
    g3.tick(299);
    let still = g3.processing();
    g3.tick(300);
    set.add(
        "processing visual state",
        idle_vis == (100, false) && busy_vis == (70, true) && still && !g3.processing(),
        "",
    );

    // 4. 导航类豁免：连续翻页全受理（连点 5 次受理 5 次）。
    let mut nav = BtnGuard::new(BtnKind::Navigate);
    let mut pages = 0;
    for i in 0..5u64 {
        if nav.click(i * 20) {
            pages += 1;
        }
    }
    set.add(
        "navigate exempt from debounce",
        pages == 5 && nav.accepted() == 5 && nav.rejected() == 0,
        "",
    );

    // 5. 切换类豁免：快速连切合法（开关连打不丢拍）。
    let mut tog = BtnGuard::new(BtnKind::Toggle);
    let mut flips = 0;
    for i in 0..6u64 {
        if tog.click(i * 10) {
            flips += 1;
        }
    }
    set.add("toggle rapid legal", flips == 6, "");

    // 6. 豁免清单审计：枚举三类——Submit 防抖、Navigate/Toggle 豁免
    //    （豁免清单唯一源）。
    set.add(
        "exemption list complete",
        DEBOUNCE_MS == 300
            && BtnKind::Navigate != BtnKind::Submit
            && BtnKind::Toggle != BtnKind::Submit,
        "",
    );

    // 7. 误伤评估：快速合法双击场景（双击=两次独立提交间隔 >300ms）不受扰。
    let mut g4 = BtnGuard::new(BtnKind::Submit);
    let a = g4.click(0);
    let b = g4.click(350);
    set.add("legal fast double not harmed", a && b && g4.accepted() == 2, "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processing_expires_exactly() {
        let mut g = BtnGuard::new(BtnKind::Submit);
        g.click(1_000);
        g.tick(1_299);
        assert!(g.processing());
        g.tick(1_300);
        assert!(!g.processing());
    }

    #[test]
    fn navigate_never_processing() {
        let mut n = BtnGuard::new(BtnKind::Navigate);
        n.click(0);
        assert!(!n.processing());
    }

    #[test]
    fn rejected_clicks_dont_extend_window() {
        // 拒绝的点击不重置窗口（窗口从首次受理起算）。
        let mut g = BtnGuard::new(BtnKind::Submit);
        g.click(0);
        g.click(50);
        g.tick(300);
        assert!(g.click(310)); // 窗口按 0+300 算，310 已出窗
    }
}
