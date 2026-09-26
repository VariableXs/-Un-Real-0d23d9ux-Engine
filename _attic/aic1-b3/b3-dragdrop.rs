
// ---------------------------------------------------------------------------
// F018 · 深化批次三：跨表面拖拽预览供给面（预览「文件名+图标」由发起方供给，
// 翻译层保证预览不缺席）
//
// 主册依据（G-A-18【交互设计】）：「跨表面拖拽时数据预览（文件名+图标）由
// 发起方供给，翻译层保证预览不缺席」。既有面：五态状态机/三取消/视觉参数/
// 死拖超时/延迟渲染/源崩溃回收全由批次一/二承载（一处一事实），本段补预览
// 供给的缺席审计（缺席 = 缺陷，计数可见）。
// ---------------------------------------------------------------------------

/// 预览供给审计（翻译层的「不缺席」承诺观测面）。
#[derive(Clone, Copy, Debug)]
pub struct PreviewCarrier {
    /// 发起方成功供给的预览数（文件名非空 + 图标可取）。
    pub supplied: u32,
    /// 预览缺席数（发起方没给/给空——缺席即缺陷，如实计数不静默）。
    pub missing: u32,
}

impl PreviewCarrier {
    pub const fn new() -> PreviewCarrier {
        PreviewCarrier { supplied: 0, missing: 0 }
    }

    /// 发起方一次预览供给：文件名与图标任一缺席记缺席（true = 供给合格）。
    pub fn offer(&mut self, name_nonempty: bool, icon_ok: bool) -> bool {
        if name_nonempty && icon_ok {
            self.supplied += 1;
            true
        } else {
            self.missing += 1;
            false
        }
    }

    /// 「预览不缺席」判据：本会话缺席恒 0 才算承诺兑现。
    pub fn gap_free(&self) -> bool {
        self.missing == 0
    }
}

/// F018 深化批次三自检。
pub fn run_dragdrop_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F018-dragdrop-deep2");
    // 1) 供给合格计数：文件名+图标双齐 = 供给；单缺 = 缺席计数。
    let mut c = PreviewCarrier::new();
    let ok1 = c.offer(true, true);
    let ok2 = c.offer(true, false);
    cs.add(
        "preview_carrier_offer_accounting",
        ok1 && !ok2 && c.supplied == 1 && c.missing == 1,
        "",
    );
    // 2) 全齐会话 gap-free；有缺席的会话如实红（缺席 = 缺陷承诺面）。
    let mut c2 = PreviewCarrier::new();
    for _ in 0..5 {
        c2.offer(true, true);
    }
    cs.add(
        "preview_gap_free_promise",
        c2.gap_free() && c2.supplied == 5 && !c.gap_free(),
        "",
    );
    cs
}
