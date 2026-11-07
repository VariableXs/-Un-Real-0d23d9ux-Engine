//! UNX-F51537 B17 多拓扑压力（×3 HUB 级联 ×8 设备全开） · 0xC0940段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51538-J1 ×3 波饥饿零发生证明
//!
//! 设计要点：
//!   ① 拓扑冻结：3 台 HUB 级联，8 台设备静态挂接（0-2→HUB0，3-5→HUB1，
//!      6-7→HUB2）；每轮每 HUB 预算独立、根预算兜底。
//!   ② 仲裁语义 grant = min(需求+欠账, 设备档, HUB 余额, 根余额)：
//!      池余额截断必须落账，零授予笔也要落账（grant_ops 含零 grant）。
//!   ③ 波次状态机：×3 波只进不退，波内 8 轮负载 + 收尾清欠轮，波末
//!      欠账必须清零（拓扑账 demand = granted + 欠账守恒）。
//!   ④ 零 hang：每轮三台 HUB 仲裁全部完成（预算不下穿），hang 计数恒 0；
//!      零饿死：每波每设备授予笔数 ≥1，饿死事件恒 0。
//!   ⑤ 注入三类必抓 ×10：超预算伪造授予 / 幽灵设备 / 账面回退，
//!      注入账守恒 注入 = 检出 + 逃逸，逃逸恒 0。
//!   ⑥ 判定次序：波相位 → 设备域 → 需求界 → 预算截断 → 落账。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// 段基址：0xC0880 + 0x40 × (51537 − 51534)。
pub const SEG_BASE: u32 = 0xC0940;

/// HUB 数（×3 级联）。
pub const HUBS: usize = 3;
/// 设备数（×8 全开）。
pub const DEVICES: usize = 8;
/// 压力波数（×3 波）。
pub const WAVES: usize = 3;
/// 每波负载轮数。
pub const ROUNDS_PER_WAVE: usize = 8;
/// 单设备单轮授予上限。
pub const DEV_CAP: u32 = 40;
/// 每轮每 HUB 预算。
pub const HUB_BUDGET: u32 = 100;
/// 每轮根预算（兜底）。
pub const ROOT_BUDGET: u32 = 512;
/// 单轮需求上限（含）。
pub const WANT_MAX: u32 = 64;
/// 单设备欠账上限。
pub const DEFERRED_MAX: u64 = 1024;
/// 收尾清欠轮数上限。
pub const DRAIN_ROUNDS_MAX: usize = 16;

/// 设备 → HUB 静态挂接表（冻结拓扑）。
pub const DEVICE_HUB: [usize; DEVICES] = [0, 0, 0, 1, 1, 1, 2, 2];

/// 诊断码闭集（多拓扑压力域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HubCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 需求为负
    WantNegative,
    /// 需求越上限
    WantTooBig,
    /// 幽灵设备（负值或超拓扑）
    DevGhost,
    /// 伪造授予超预算
    OverBudget,
    /// 账面回退
    LedgerRewrite,
    /// 波次回退或越界
    WaveRegress,
    /// 欠账越上限
    DeferredCapExceeded,
    /// 注入账守恒失败
    LedgerMismatch,
}

impl HubCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HubCode::Ok => "成功",
            HubCode::SegBad => "段位越界：核对 SEG_BASE 对齐与取值范围",
            HubCode::WantNegative => "需求为负：带宽需求必须 ≥0",
            HubCode::WantTooBig => "需求越界：单轮需求不得超过上限",
            HubCode::DevGhost => "幽灵设备：设备号必须落在冻结拓扑内",
            HubCode::OverBudget => "超预算：授予不得超过档位与池余额",
            HubCode::LedgerRewrite => "账面回退：授予总量只增不减",
            HubCode::WaveRegress => "波次回退：波次状态机只进不退",
            HubCode::DeferredCapExceeded => "欠账越上限：清欠后再压",
            HubCode::LedgerMismatch => "注入账不平：注入=检出+逃逸必须守恒",
        }
    }
}

/// 单轮报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoundReport {
    pub granted: u64,
    pub grant_ops: u64,
    pub deferred_outstanding: u64,
}

/// 单波报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaveReport {
    pub wave_idx: usize,
    pub granted: u64,
    pub deferred_left: u64,
    pub drain_rounds: usize,
}

/// HUB 级联压力引擎：预算仲裁 + 波次机 + 注入账。
pub struct HubStress {
    deferred: [u64; DEVICES],
    granted_total: u64,
    demand_total: u64,
    grant_ops: u64,
    wave: usize,
    wave_granted: [[u64; DEVICES]; WAVES],
    hang_rounds: u32,
    starve_events: u32,
    lcg: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl HubStress {
    pub fn new() -> HubStress {
        HubStress {
            deferred: [0; DEVICES],
            granted_total: 0,
            demand_total: 0,
            grant_ops: 0,
            wave: 0,
            wave_granted: [[0; DEVICES]; WAVES],
            hang_rounds: 0,
            starve_events: 0,
            lcg: 0x5153_7000_C094_0001,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    fn lcg_next(&mut self) -> u64 {
        self.lcg = self
            .lcg
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.lcg
    }

    /// 欠账总额。
    pub fn deferred_outstanding(&self) -> u64 {
        let mut sum = 0u64;
        for d in 0..DEVICES {
            let v = match self.deferred.get(d) {
                Some(v) => *v,
                None => 0,
            };
            sum += v;
        }
        sum
    }

    /// 拓扑账守恒：需求 = 授予 + 欠账（欠账只延不灭）。
    pub fn topo_consistent(&self) -> bool {
        self.demand_total == self.granted_total + self.deferred_outstanding()
    }

    /// 仲裁一轮：显式需求向量（判序：设备域 → 需求界 → 预算截断 → 落账）。
    pub fn arbitrate_round(&mut self, wants: &[i64]) -> Result<RoundReport, HubCode> {
        if wants.len() > DEVICES {
            return Err(HubCode::DevGhost);
        }
        let mut hub_remaining = [HUB_BUDGET; HUBS];
        let mut root_remaining = ROOT_BUDGET;
        let mut granted = 0u64;
        let mut ops = 0u64;
        for d in 0..DEVICES {
            let want = match wants.get(d) {
                Some(w) => *w,
                None => 0,
            };
            let hub = match DEVICE_HUB.get(d) {
                Some(h) => *h,
                None => return Err(HubCode::DevGhost),
            };
            if want < 0 {
                return Err(HubCode::WantNegative);
            }
            if want as u64 > WANT_MAX as u64 {
                return Err(HubCode::WantTooBig);
            }
            let cur_deferred = match self.deferred.get(d) {
                Some(v) => *v,
                None => return Err(HubCode::DevGhost),
            };
            let demand = match (want as u64).checked_add(cur_deferred) {
                Some(v) => v,
                None => return Err(HubCode::DeferredCapExceeded),
            };
            let mut grant = demand.min(DEV_CAP as u64);
            let hub_rem = match hub_remaining.get(hub) {
                Some(r) => *r as u64,
                None => return Err(HubCode::DevGhost),
            };
            if grant > hub_rem {
                grant = hub_rem;
            }
            if grant > root_remaining as u64 {
                grant = root_remaining as u64;
            }
            let new_deferred = demand - grant;
            if new_deferred > DEFERRED_MAX {
                return Err(HubCode::DeferredCapExceeded);
            }
            // 需求账只记新鲜需求（欠账已在入账轮记过，不重复计）。
            self.demand_total += want as u64;
            match self.deferred.get_mut(d) {
                Some(v) => *v = new_deferred,
                None => return Err(HubCode::DevGhost),
            }
            match hub_remaining.get_mut(hub) {
                Some(r) => *r = (*r as u64 - grant) as u32,
                None => return Err(HubCode::DevGhost),
            }
            root_remaining -= grant as u32;
            // 零授予笔也落账。
            self.grant_ops += 1;
            ops += 1;
            self.granted_total += grant;
            granted += grant;
            if self.wave < WAVES {
                match self.wave_granted.get_mut(self.wave) {
                    Some(wg) => {
                        let slot = match wg.get_mut(d) {
                            Some(s) => s,
                            None => return Err(HubCode::DevGhost),
                        };
                        *slot += grant;
                    }
                    None => return Err(HubCode::WaveRegress),
                }
            }
        }
        if !self.topo_consistent() {
            self.hang_rounds += 1;
        }
        Ok(RoundReport {
            granted,
            grant_ops: ops,
            deferred_outstanding: self.deferred_outstanding(),
        })
    }

    /// 一波压力：8 轮 LCG 需求（每轮每设备 ≥8）+ 收尾清欠；波末欠账清零。
    pub fn run_wave(&mut self) -> Result<WaveReport, HubCode> {
        if self.wave >= WAVES {
            return Err(HubCode::WaveRegress);
        }
        for _ in 0..ROUNDS_PER_WAVE {
            let mut wants: [i64; DEVICES] = [0; DEVICES];
            for d in 0..DEVICES {
                let v = (self.lcg_next() % 57) as i64 + 8;
                match wants.get_mut(d) {
                    Some(s) => *s = v,
                    None => return Err(HubCode::DevGhost),
                }
            }
            self.arbitrate_round(&wants)?;
        }
        // 收尾清欠：无新需求，只还欠账。
        let mut drain_rounds = 0usize;
        while self.deferred_outstanding() > 0 && drain_rounds < DRAIN_ROUNDS_MAX {
            let zeros: [i64; DEVICES] = [0; DEVICES];
            self.arbitrate_round(&zeros)?;
            drain_rounds += 1;
        }
        // 饿死判定：本波每设备授予必须 ≥1。
        let mut starved = false;
        if let Some(wg) = self.wave_granted.get(self.wave) {
            for d in 0..DEVICES {
                let g = match wg.get(d) {
                    Some(v) => *v,
                    None => 0,
                };
                if g == 0 {
                    starved = true;
                }
            }
        }
        if starved {
            self.starve_events += 1;
        }
        let report = WaveReport {
            wave_idx: self.wave,
            granted: self.granted_total,
            deferred_left: self.deferred_outstanding(),
            drain_rounds,
        };
        self.wave += 1;
        Ok(report)
    }

    /// 波次回退注入面：任何回退/越位请求一律显性拒，状态不落。
    pub fn try_set_wave(&mut self, target: usize) -> Result<usize, HubCode> {
        if target >= self.wave || target >= WAVES {
            return Err(HubCode::WaveRegress);
        }
        Err(HubCode::WaveRegress)
    }

    /// 注入 ①：伪造授予（绕过仲裁，超档位必抓）。
    pub fn forge_grant(&mut self, dev: usize, amount: u32) -> Result<(), HubCode> {
        if dev >= DEVICES {
            return Err(HubCode::DevGhost);
        }
        if amount as u64 > DEV_CAP as u64 {
            return Err(HubCode::OverBudget);
        }
        match self.deferred.get_mut(dev) {
            Some(v) => *v = v.saturating_sub(amount as u64),
            None => return Err(HubCode::DevGhost),
        }
        self.granted_total += amount as u64;
        Ok(())
    }

    /// 注入 ③：账面回退（授予总量只增不减）。
    pub fn try_rewind_granted(&mut self, target: u64) -> Result<(), HubCode> {
        if target < self.granted_total {
            return Err(HubCode::LedgerRewrite);
        }
        Ok(())
    }

    pub fn wave(&self) -> usize {
        self.wave
    }

    pub fn granted_total(&self) -> u64 {
        self.granted_total
    }

    pub fn demand_total(&self) -> u64 {
        self.demand_total
    }

    pub fn grant_ops(&self) -> u64 {
        self.grant_ops
    }

    pub fn hang_rounds(&self) -> u32 {
        self.hang_rounds
    }

    pub fn starve_events(&self) -> u32 {
        self.starve_events
    }

    pub fn note_injected(&mut self) {
        self.injected += 1;
    }

    pub fn note_detected(&mut self) {
        self.detected += 1;
    }

    pub fn note_escaped(&mut self) {
        self.escaped += 1;
    }

    pub fn injected(&self) -> u64 {
        self.injected
    }

    pub fn detected(&self) -> u64 {
        self.detected
    }

    pub fn escaped(&self) -> u64 {
        self.escaped
    }

    /// 注入账守恒：注入 = 检出 + 逃逸，逃逸恒 0、检出非零。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }
}

/// 便捷构造：跑满 ×3 波全开压力。
pub fn run_three_waves() -> HubStress {
    let mut st = HubStress::new();
    for _ in 0..WAVES {
        let _ = st.run_wave();
    }
    st
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一 LCG 种子两次跑波，账面逐字段一致。
pub fn dual_run_consistent() -> bool {
    let a = run_three_waves();
    let b = run_three_waves();
    a.granted_total() == b.granted_total()
        && a.demand_total() == b.demand_total()
        && a.grant_ops() == b.grant_ops()
        && a.topo_consistent()
        && b.topo_consistent()
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51537_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51537");
    // 1 正常流：×3 波全开，波末欠账清零。
    {
        let mut st = HubStress::new();
        let mut ok = true;
        for w in 0..WAVES {
            match st.run_wave() {
                Ok(rep) => {
                    if rep.wave_idx != w || rep.deferred_left != 0 {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
        }
        s.add_dyn("×3 波全开压满", ok, "三波依次压满且波末欠账清零");
    }
    // 2 边界-零：全零需求轮——零授予笔也落账，账面守恒。
    {
        let mut st = HubStress::new();
        let zeros: [i64; DEVICES] = [0; DEVICES];
        let r = st.arbitrate_round(&zeros);
        let zero_ok = match r {
            Ok(rep) => rep.granted == 0 && rep.grant_ops == DEVICES as u64,
            Err(_) => false,
        };
        s.add_dyn(
            "边界零授予落账",
            zero_ok && st.topo_consistent() && st.granted_total() == 0,
            "零需求轮 8 笔全部落账且账面守恒",
        );
    }
    // 3 边界-负：负需求显性拒。
    {
        let mut st = HubStress::new();
        let mut wants: [i64; DEVICES] = [0; DEVICES];
        let _ = wants.get_mut(2).map(|w| *w = -1);
        let r = st.arbitrate_round(&wants);
        s.add_dyn("边界负值拒", r == Err(HubCode::WantNegative), "负需求必须显性拒");
    }
    // 4 边界-上限：需求取 WANT_MAX 放行（走欠账通道）。
    {
        let mut st = HubStress::new();
        let wants: [i64; DEVICES] = [WANT_MAX as i64; DEVICES];
        let r = st.arbitrate_round(&wants);
        s.add_dyn("边界上限放行", r.is_ok(), "上限需求走预算截断与欠账");
    }
    // 5 边界-上限+1：需求越界拒。
    {
        let mut st = HubStress::new();
        let mut wants: [i64; DEVICES] = [0; DEVICES];
        let _ = wants.get_mut(0).map(|w| *w = WANT_MAX as i64 + 1);
        let r = st.arbitrate_round(&wants);
        s.add_dyn("边界上限+1 拒", r == Err(HubCode::WantTooBig), "越界需求必须拒绝");
    }
    // 6 注入必抓 ①：超预算伪造授予 ×10 全抓。
    {
        let mut st = HubStress::new();
        let mut caught = 0u32;
        for k in 0..10u32 {
            st.note_injected();
            match st.forge_grant(0, DEV_CAP + 1 + k) {
                Err(HubCode::OverBudget) => {
                    caught += 1;
                    st.note_detected();
                }
                _ => st.note_escaped(),
            }
        }
        s.add_dyn("超预算注入 10/10", caught == 10, "绕过仲裁的超档授予必须全抓");
    }
    // 7 注入必抓 ②：幽灵设备 ×10 全抓。
    {
        let mut st = HubStress::new();
        let mut caught = 0u32;
        for k in 0..10u32 {
            st.note_injected();
            match st.forge_grant(DEVICES + k as usize, 8) {
                Err(HubCode::DevGhost) => {
                    caught += 1;
                    st.note_detected();
                }
                _ => st.note_escaped(),
            }
        }
        s.add_dyn("幽灵设备注入 10/10", caught == 10, "拓扑外设备必须全抓");
    }
    // 8 注入必抓 ③：账面回退 ×10 全抓。
    {
        let mut st = run_three_waves();
        let total = st.granted_total();
        let mut caught = 0u32;
        for k in 0..10u32 {
            st.note_injected();
            match st.try_rewind_granted(total.saturating_sub(1 + k as u64)) {
                Err(HubCode::LedgerRewrite) => {
                    caught += 1;
                    st.note_detected();
                }
                _ => st.note_escaped(),
            }
        }
        s.add_dyn("账面回退注入 10/10", caught == 10, "授予总量只增不减");
    }
    // 9 判定次序：设备域先于预算界（幽灵设备+超档同传报幽灵）。
    {
        let mut st = HubStress::new();
        let r = st.forge_grant(DEVICES + 5, DEV_CAP + 99);
        s.add_dyn(
            "判序设备先于预算",
            r == Err(HubCode::DevGhost),
            "幽灵设备判据先于超预算判据",
        );
    }
    // 10 状态机拒止：波次回退与越位显性拒；超波压入拒。
    {
        let mut st = HubStress::new();
        let _ = st.run_wave();
        let r1 = st.try_set_wave(0);
        let r2 = st.try_set_wave(9);
        let mut st2 = run_three_waves();
        let r3 = st2.run_wave();
        s.add_dyn(
            "波次机拒止",
            r1 == Err(HubCode::WaveRegress)
                && r2 == Err(HubCode::WaveRegress)
                && r3 == Err(HubCode::WaveRegress),
            "波次回退/越位/超波必须全部拒绝",
        );
    }
    // 11 预算截断：满额需求轮出现池余额截断（授予 < 需求），账仍守恒。
    {
        let mut st = HubStress::new();
        let wants: [i64; DEVICES] = [WANT_MAX as i64; DEVICES];
        let r = st.arbitrate_round(&wants);
        let truncated = match r {
            Ok(rep) => rep.granted < (DEVICES * WANT_MAX as usize) as u64 && rep.deferred_outstanding > 0,
            Err(_) => false,
        };
        s.add_dyn(
            "池余额截断落账",
            truncated && st.topo_consistent(),
            "grant=min(需求+欠账,档位,余额) 截断差入欠账",
        );
    }
    // 12 拓扑账一致：×3 波全程 demand = granted + 欠账，波末欠账 0。
    {
        let st = run_three_waves();
        s.add_dyn(
            "拓扑账一致",
            st.topo_consistent()
                && st.demand_total() == st.granted_total()
                && st.deferred_outstanding() == 0,
            "三波总账守恒且收尾清欠彻底",
        );
    }
    // 13 ×3 波零 hang 零饿死。
    {
        let st = run_three_waves();
        s.add_dyn(
            "×3 波零 hang 零饿死",
            st.hang_rounds() == 0 && st.starve_events() == 0,
            "三波 hang 计数与饿死事件都必须为 0",
        );
    }
    // 14 对账守恒：注入 = 检出 + 逃逸，逃逸 0。
    {
        let mut st = run_three_waves();
        for k in 0..10u32 {
            st.note_injected();
            match st.forge_grant(0, DEV_CAP + 1 + k) {
                Err(HubCode::OverBudget) => st.note_detected(),
                _ => st.note_escaped(),
            }
        }
        s.add_dyn(
            "注入账守恒",
            st.conserved() && st.injected() == 10,
            "注入=检出+逃逸，逃逸恒 0",
        );
    }
    // 15 双跑一致。
    s.add_dyn("双跑账面一致", dual_run_consistent(), "同种子两次跑波账面全等");
    // 16 文案闭集非空 + 段位守卫。
    {
        let mut closed = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [HubCode; 10] = [
    HubCode::Ok,
    HubCode::SegBad,
    HubCode::WantNegative,
    HubCode::WantTooBig,
    HubCode::DevGhost,
    HubCode::OverBudget,
    HubCode::LedgerRewrite,
    HubCode::WaveRegress,
    HubCode::DeferredCapExceeded,
    HubCode::LedgerMismatch,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_waves_full_load() {
        let st = run_three_waves();
        assert_eq!(st.wave(), WAVES);
        assert_eq!(st.deferred_outstanding(), 0);
        assert_eq!(st.demand_total(), st.granted_total());
        assert!(st.topo_consistent());
    }

    #[test]
    fn zero_want_round_still_ledgered() {
        let mut st = HubStress::new();
        let zeros: [i64; DEVICES] = [0; DEVICES];
        let rep = match st.arbitrate_round(&zeros) {
            Ok(r) => r,
            Err(_) => {
                assert!(false, "零需求轮不应失败");
                return;
            }
        };
        assert_eq!(rep.grant_ops, DEVICES as u64);
        assert_eq!(rep.granted, 0);
        assert!(st.topo_consistent());
    }

    #[test]
    fn negative_and_overflow_want_rejected() {
        let mut st = HubStress::new();
        let mut neg: [i64; DEVICES] = [0; DEVICES];
        let _ = neg.get_mut(1).map(|w| *w = -1);
        assert_eq!(st.arbitrate_round(&neg), Err(HubCode::WantNegative));
        let mut big: [i64; DEVICES] = [0; DEVICES];
        let _ = big.get_mut(1).map(|w| *w = WANT_MAX as i64 + 1);
        assert_eq!(st.arbitrate_round(&big), Err(HubCode::WantTooBig));
        let max: [i64; DEVICES] = [WANT_MAX as i64; DEVICES];
        assert!(st.arbitrate_round(&max).is_ok());
    }

    #[test]
    fn budget_truncation_creates_deferred() {
        let mut st = HubStress::new();
        let wants: [i64; DEVICES] = [WANT_MAX as i64; DEVICES];
        let rep = match st.arbitrate_round(&wants) {
            Ok(r) => r,
            Err(_) => {
                assert!(false, "满额轮不应失败");
                return;
            }
        };
        assert!(rep.granted < (DEVICES * WANT_MAX as usize) as u64);
        assert!(rep.deferred_outstanding > 0);
        assert!(st.topo_consistent());
    }

    #[test]
    fn injection_over_budget_caught_ten() {
        let mut st = HubStress::new();
        let mut caught = 0u32;
        for k in 0..10u32 {
            st.note_injected();
            if st.forge_grant(0, DEV_CAP + 1 + k) == Err(HubCode::OverBudget) {
                caught += 1;
                st.note_detected();
            } else {
                st.note_escaped();
            }
        }
        assert_eq!(caught, 10);
        assert!(st.conserved());
    }

    #[test]
    fn injection_ghost_device_and_judge_order() {
        let mut st = HubStress::new();
        for k in 0..10u32 {
            st.note_injected();
            if st.forge_grant(DEVICES + k as usize, 8) == Err(HubCode::DevGhost) {
                st.note_detected();
            } else {
                st.note_escaped();
            }
        }
        assert!(st.conserved());
        assert_eq!(
            st.forge_grant(DEVICES + 5, DEV_CAP + 99),
            Err(HubCode::DevGhost)
        );
    }

    #[test]
    fn ledger_rewind_rejected() {
        let mut st = run_three_waves();
        let total = st.granted_total();
        assert!(total > 0);
        assert_eq!(
            st.try_rewind_granted(total - 1),
            Err(HubCode::LedgerRewrite)
        );
        assert_eq!(st.try_rewind_granted(total), Ok(()));
    }

    #[test]
    fn wave_machine_rejects_regress_and_overflow() {
        let mut st = HubStress::new();
        let _ = st.run_wave();
        assert_eq!(st.try_set_wave(0), Err(HubCode::WaveRegress));
        assert_eq!(st.try_set_wave(9), Err(HubCode::WaveRegress));
        let mut st2 = run_three_waves();
        assert_eq!(st2.run_wave(), Err(HubCode::WaveRegress));
    }

    #[test]
    fn zero_hang_zero_starve_three_waves() {
        let st = run_three_waves();
        assert_eq!(st.hang_rounds(), 0);
        assert_eq!(st.starve_events(), 0);
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_and_closed_texts() {
        assert!(seg_guard_ok());
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
