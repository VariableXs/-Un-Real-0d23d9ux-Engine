
// ---------------------------------------------------------------------------
// F001 · 深化批次七：会话表背压（满容诚实拒绝——新双击不静默丢失）
//
// 主册依据（G-A-01【设计细节】）：「会话表容量（常用 50 件同时活跃装载上限
// 远低于此；背压如实上抛）」——背压是承诺：第 65 个并发装载如实拒绝（配
// 归因短语），不静默排队到丢。
// ---------------------------------------------------------------------------

/// 会话背压闸（active/容量/refused 计数）。
#[derive(Clone, Copy, Debug)]
pub struct SessionBackpressure {
    pub active: u32,
    pub refused: u32,
}

impl SessionBackpressure {
    pub const fn new() -> SessionBackpressure {
        SessionBackpressure { active: 0, refused: 0 }
    }

    /// 尝试开始装载：容量内 → 占位（true）；满容 → 拒绝（false + 计数——
    /// 上抛面给三要素：「装载服务满载，请稍后重试」）。
    pub fn try_begin(&mut self, cap: u32) -> bool {
        if self.active >= cap {
            self.refused += 1;
            return false;
        }
        self.active += 1;
        true
    }

    /// 装载终态释放槽位（saturating——防御式不减至负）。
    pub fn end(&mut self) {
        self.active = self.active.saturating_sub(1);
    }

    /// 满载归因短语（三要素之「发生了什么+为什么」；下一步 = 稍后重试）。
    pub const REFUSAL_PHRASE: &'static str = "装载服务满载，请稍后重试";
}

/// F001 深化批次七自检。
pub fn run_dblrun_deep6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F001-dblrun-deep6");
    // 1) 容量内全过；第 cap+1 个如实拒并计数（背压可见）。
    let mut bp = SessionBackpressure::new();
    let mut all_ok = true;
    for _ in 0..64 {
        all_ok &= bp.try_begin(64);
    }
    let over = bp.try_begin(64);
    cs.add(
        "backpressure_cap_refusal",
        all_ok && !over && bp.refused == 1 && bp.active == 64,
        "",
    );
    // 2) 终态释放槽位后可再入（拒绝不是永久判死——稍后重试成立）。
    bp.end();
    let re = bp.try_begin(64);
    cs.add(
        "backpressure_slot_reusable",
        re && bp.refused == 1,
        "",
    );
    // 3) 归因短语非空（三要素锚——不裸拒）。
    cs.add(
        "backpressure_refusal_phrase",
        !SessionBackpressure::REFUSAL_PHRASE.is_empty(),
        "",
    );
    cs
}
