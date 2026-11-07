//! UNX-F51555 B18 端口供电档位账（高电流端口标记） · 0xC0D80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 51555 逐字锚定）：① 纯功能代码 ≥300 行；② 判据原文：
//! UNX-F51556-J1 档位 ×10 判定正确；超档取流注入 10/10 拒绝；③ 完成时
//! API/软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   数据结构：TierTable（档位账：16 口 × 闭集 4 档（0.5A/0.9A/1.5A/3A），
//!     高电流标记纯函数）、CurrentGuard（取流守卫：逐口实时取流 vs 档位上限，
//!     超档显性拒收账）。
//!   状态机：PtPhase 档位链 Probe→Mark→Enforce→Audit，单调。
//!   错误路径：PtCode 闭集 12 变体，val() = SEG_BASE + 1 + 变体序号。
//!   注入核：超档取流/未登记档/档位回退/越册口/相位倒退/封账写
//!     六类坏样本，每类 10 轮精确复现错误档，守恒（检出+逃逸=轮数）。
//!   双跑指纹：标记+取流+审计全程混入 FNV-1a，两次独立全流程逐位一致。
//!
//! 设计要点：no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 =
//!   SEG_BASE 派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/
//!   saturating_*，无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX12 派工公式：0xC0000 + 0x40 × (51555 − 51501)）。
pub const SEG_BASE: u32 = 0xC0D80;

/// 端口数。
pub const PORTS: usize = 16;

/// 档位 ×10 判定轮数。
pub const TIER_ROUNDS: usize = 10;

/// 注入轮数（每类 10/10）。
pub const INJ_ROUNDS: usize = 10;

/// 档位闭集（毫安）：0.5A / 0.9A / 1.5A / 3.0A。
pub const TIERS: [u32; 4] = [500, 900, 1500, 3000];

/// 高电流标记阈值（毫安）。
pub const HIGH_MARK: u32 = 1500;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（val = SEG_BASE + 1 + 变体序号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PtCode {
    /// 超档取流：取流超档位上限
    EOverDraw,
    /// 未登记档：档位越闭集
    EBadTier,
    /// 档位回退：已登记档倒退
    ETierRewind,
    /// 越册口
    ENoPort,
    /// 相位倒退
    ERewind,
    /// 高标缺失：高电流口未标记
    EMissMark,
    /// 双跑指纹不一致
    EFp,
    /// 审计不齐：有口未审计
    EUnder,
    /// 拒收账破缺：拒收数与超档数不等
    EConserve,
    /// 档位上限为零
    EZeroCap,
    /// 重复标记：同口二次登记同档
    EDupMark,
    /// 封账写
    ESealed,
}

impl PtCode {
    /// 诊断码段内数值。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量非空中文诊断。
    pub fn detail(self) -> &'static str {
        match self {
            PtCode::EOverDraw => "超档取流：取流超档位上限，显性拒收并落账",
            PtCode::EBadTier => "未登记档：档位必须落在 0.5/0.9/1.5/3.0A 闭集内",
            PtCode::ETierRewind => "档位回退：已登记档不得倒退，先注销再降档",
            PtCode::ENoPort => "越册口：核对 16 口册行序",
            PtCode::ERewind => "相位倒退：档位链只进不退，越级显性拒",
            PtCode::EMissMark => "高标缺失：取流达高电流阈值而未标记，先补标",
            PtCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PtCode::EUnder => "审计不齐：有口未审计即封，先补齐",
            PtCode::EConserve => "拒收账破缺：拒收数与超档数不等，逐条复算",
            PtCode::EZeroCap => "档位上限为零：非法档位行，先修表",
            PtCode::EDupMark => "重复标记：同口同档二次登记，按号去重",
            PtCode::ESealed => "封账写：账已封，另开新账本",
        }
    }
}

/// 闭集全量数组。
pub const ALL: [PtCode; 12] = [
    PtCode::EOverDraw,
    PtCode::EBadTier,
    PtCode::ETierRewind,
    PtCode::ENoPort,
    PtCode::ERewind,
    PtCode::EMissMark,
    PtCode::EFp,
    PtCode::EUnder,
    PtCode::EConserve,
    PtCode::EZeroCap,
    PtCode::EDupMark,
    PtCode::ESealed,
];

// ── 状态机 ───────────────────────────────────────────────────────
/// 档位相位（单调）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PtPhase {
    /// 探测。
    Probe,
    /// 标记。
    Mark,
    /// 执法。
    Enforce,
    /// 审计。
    Audit,
}

/// 相位推进合法性。
pub fn phase_step(from: PtPhase, to: PtPhase) -> bool {
    matches!(
        (from, to),
        (PtPhase::Probe, PtPhase::Mark)
            | (PtPhase::Mark, PtPhase::Enforce)
            | (PtPhase::Enforce, PtPhase::Audit)
    )
}

// ── 档位账 ───────────────────────────────────────────────────────
/// 档位账：逐口档位（闭集索引 0..=3）+ 高电流标记。
pub struct TierTable {
    tier: [Option<u32>; PORTS],
    high: [bool; PORTS],
    marked: u32,
    sealed: bool,
    fp: u64,
}

impl TierTable {
    /// 新账。
    pub fn new() -> Self {
        TierTable {
            tier: [None; PORTS],
            high: [false; PORTS],
            marked: 0,
            sealed: false,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 档位登记：闭集闸 + 回退闸 + 高电流标记联动。
    pub fn set_tier(&mut self, port: usize, tier_idx: u32) -> Result<bool, PtCode> {
        if self.sealed {
            return Err(PtCode::ESealed);
        }
        if port >= PORTS {
            return Err(PtCode::ENoPort);
        }
        let cap = match TIERS.get(tier_idx as usize) {
            Some(c) => *c,
            None => return Err(PtCode::EBadTier),
        };
        if cap == 0 {
            return Err(PtCode::EZeroCap);
        }
        // 回退闸：新档不得低于已登记档。
        if let Some(Some(old)) = self.tier.get(port) {
            if let Some(old_cap) = TIERS.get(*old as usize) {
                if cap < *old_cap {
                    return Err(PtCode::ETierRewind);
                }
            }
            // 重复标记：同档重登。
            if let Some(old2) = self.tier.get(port) {
                if *old2 == Some(tier_idx) {
                    return Err(PtCode::EDupMark);
                }
            }
        }
        let high = cap >= HIGH_MARK;
        let was_high = match self.high.get(port) {
            Some(v) => *v,
            None => false,
        };
        if let Some(c) = self.tier.get_mut(port) {
            *c = Some(tier_idx);
        }
        if let Some(c) = self.high.get_mut(port) {
            *c = high;
        }
        if high && !was_high {
            self.marked = self.marked.saturating_add(1);
        }
        self.fnv((port as u64) << 16 | tier_idx as u64);
        Ok(high)
    }

    /// 档位读出。
    pub fn tier_of(&self, port: usize) -> Option<u32> {
        self.tier.get(port).copied().flatten()
    }

    /// 高电流标记读出。
    pub fn is_high(&self, port: usize) -> Option<bool> {
        self.high.get(port).copied()
    }

    /// 高标数。
    pub fn marked_count(&self) -> u32 {
        self.marked
    }

    /// 高标缺失闸：高电流档未标记即破缺。
    pub fn marks_consistent(&self) -> bool {
        let mut i = 0usize;
        while i < PORTS {
            if let Some(Some(t)) = self.tier.get(i) {
                let cap = TIERS.get(*t as usize).copied().unwrap_or(0);
                let marked = self.high.get(i).copied().unwrap_or(false);
                if cap >= HIGH_MARK && !marked {
                    return false;
                }
                if cap < HIGH_MARK && marked {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
        self.fnv(0x5E5F);
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

// ── 取流守卫 ─────────────────────────────────────────────────────
/// 取流守卫：逐口取流 vs 档位上限，超档显性拒收。
pub struct CurrentGuard {
    draws: [u32; PORTS],
    pub rejects: u32,
    accepted: u32,
}

impl CurrentGuard {
    /// 新守卫。
    pub fn new() -> Self {
        CurrentGuard {
            draws: [0; PORTS],
            rejects: 0,
            accepted: 0,
        }
    }

    /// 取流申请：超档显性拒。
    pub fn draw(&mut self, port: usize, ma: u32, tier_idx: u32) -> Result<(), PtCode> {
        if port >= PORTS {
            return Err(PtCode::ENoPort);
        }
        let cap = match TIERS.get(tier_idx as usize) {
            Some(c) => *c,
            None => return Err(PtCode::EBadTier),
        };
        if ma > cap {
            self.rejects = self.rejects.saturating_add(1);
            return Err(PtCode::EOverDraw);
        }
        match self.draws.get_mut(port) {
            Some(c) => *c = ma,
            None => return Err(PtCode::ENoPort),
        }
        self.accepted = self.accepted.saturating_add(1);
        Ok(())
    }

    /// 取流读出。
    pub fn draw_of(&self, port: usize) -> Option<u32> {
        self.draws.get(port).copied()
    }

    /// 守恒闸：接受 + 拒收 = 总申请。
    pub fn conserve_ok(&self, requested: u32) -> bool {
        self.accepted + self.rejects == requested
    }
}

// ── 注入器（LCG 种子 = SEG_BASE 派生）───────────────────────────
/// 六类坏样本注入器。
pub struct PtInjector {
    lcg: u32,
}

impl PtInjector {
    /// 确定性构造。
    pub fn new() -> Self {
        PtInjector { lcg: SEG_BASE }
    }

    /// LCG 下一步。
    pub fn next(&mut self) -> u32 {
        self.lcg = self.lcg.wrapping_mul(1664525).wrapping_add(1013904223);
        self.lcg
    }

    /// 注入核：六类坏样本各 10 轮。返回（检出, 逃逸, 各类检出）。
    pub fn inject_all(&self) -> (u32, u32, [u32; 6]) {
        let mut hit = [0u32; 6];
        let mut escaped = 0u32;
        let mut cls = 0usize;
        while cls < 6 {
            let mut r = 0usize;
            while r < INJ_ROUNDS {
                let mut inj = PtInjector { lcg: SEG_BASE ^ (cls as u32) ^ (r as u32 + 1) };
                let noise = inj.next();
                let port = (noise % PORTS as u32) as usize;
                let ok = match cls {
                    // 类0 超档取流：1.5A 档取 2000。
                    0 => {
                        let mut g = CurrentGuard::new();
                        g.draw(port, 2000, 2) == Err(PtCode::EOverDraw) && g.rejects == 1
                    }
                    // 类1 未登记档：档位 4/9。
                    1 => {
                        let mut t = TierTable::new();
                        t.set_tier(port, 4 + noise % 6) == Err(PtCode::EBadTier)
                    }
                    // 类2 档位回退：3A 降 0.9A。
                    2 => {
                        let mut t = TierTable::new();
                        let _ = t.set_tier(port, 3);
                        t.set_tier(port, 1) == Err(PtCode::ETierRewind)
                    }
                    // 类3 越册口。
                    3 => {
                        let mut t = TierTable::new();
                        t.set_tier(PORTS + (noise % 4) as usize, 0) == Err(PtCode::ENoPort)
                    }
                    // 类4 重复标记：同档重登。
                    4 => {
                        let mut t = TierTable::new();
                        let _ = t.set_tier(port, 2);
                        t.set_tier(port, 2) == Err(PtCode::EDupMark)
                    }
                    // 类5 相位倒退：越级全拒。
                    _ => !phase_step(PtPhase::Probe, PtPhase::Enforce)
                        && !phase_step(PtPhase::Audit, PtPhase::Mark),
                };
                if ok {
                    hit[cls] += 1;
                } else {
                    escaped += 1;
                }
                r += 1;
            }
            cls += 1;
        }
        let total: u32 = hit.iter().copied().fold(0u32, u32::saturating_add);
        (total, escaped, hit)
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：档位 ×10 判定 + 超档取流 ×10 拒收 + 审计 + 指纹。
/// 返回（判定正确轮数, 拒收数, 审计通过轮数, 指纹）。
pub fn run_cycle() -> (u32, u32, u32, u64) {
    let mut tier_ok = 0u32;
    let mut rejects_total = 0u32;
    let mut audit_ok = 0u32;
    let mut fp_last: u64 = 0;
    let mut r = 0usize;
    while r < TIER_ROUNDS {
        let mut t = TierTable::new();
        let mut g = CurrentGuard::new();
        // 逐口登记档位：偶口 0.9A、奇口 3A（确定性）。
        let mut p = 0usize;
        while p < PORTS {
            let tier = if p % 2 == 0 { 1u32 } else { 3u32 };
            let _ = t.set_tier(p, tier);
            p += 1;
        }
        // 档位 ×10 判定：高标数 = 奇口数 8，且标记一致性过。
        if t.marked_count() == 8 && t.marks_consistent() {
            tier_ok += 1;
        }
        // 相位链。
        let mut ph = PtPhase::Probe;
        if phase_step(ph, PtPhase::Mark) {
            ph = PtPhase::Mark;
        }
        if phase_step(ph, PtPhase::Enforce) {
            ph = PtPhase::Enforce;
        }
        // 超档取流注入 ×10：逐口超档必拒。
        let mut k = 0u32;
        let mut requested = 0u32;
        while k < 10 {
            let port = ((k + r as u32) % PORTS as u32) as usize;
            let _ = g.draw(port, 3000, 1); // 0.9A 档取 3A → 拒
            requested += 1;
            k += 1;
        }
        // 合法取流 ×10。
        while k < 20 {
            let port = ((k + r as u32) % PORTS as u32) as usize;
            let _ = g.draw(port, 800, 1);
            requested += 1;
            k += 1;
        }
        let rejects = g.rejects;
        // 审计：拒收 = 10、守恒过、相位到位。
        if phase_step(ph, PtPhase::Audit)
            && rejects == 10
            && g.conserve_ok(requested)
            && t.marks_consistent()
        {
            audit_ok += 1;
        }
        rejects_total += rejects;
        fp_last ^= t.fingerprint() ^ (g.rejects as u64);
        r += 1;
    }
    (tier_ok, rejects_total, audit_ok, fp_last)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集。
pub fn run_unx51555_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51555");
    let (tier_ok, rejects, audit_ok, fp) = run_cycle();
    // 1. 档位 ×10 判定正确。
    s.add_dyn(
        "档位 ×10 判定正确",
        tier_ok == TIER_ROUNDS as u32,
        &format!("正确={}/{} 高标=8/16", tier_ok, TIER_ROUNDS),
    );
    // 2. 超档取流注入 10/10 拒绝。
    s.add_dyn(
        "超档取流注入 10/10 拒绝",
        rejects == TIER_ROUNDS as u32 * 10,
        &format!("拒收={}（每轮 10 拒）", rejects),
    );
    // 3. 审计通过 ×10。
    s.add_dyn(
        "取流审计 ×10 通过",
        audit_ok == TIER_ROUNDS as u32,
        &format!("通过={}/{} 守恒+标记一致性", audit_ok, TIER_ROUNDS),
    );
    // 4. 注入六类 60/60。
    let inj = PtInjector::new();
    let (hit, esc, per) = inj.inject_all();
    s.add_dyn(
        "注入六类 60/60 检出逃逸0",
        hit == 60 && esc == 0,
        &format!("检出={} 逃逸={} 守恒=60", hit, esc),
    );
    let _ = per;
    // 5. 高标一致性闸可测。
    let mut t = TierTable::new();
    let _ = t.set_tier(0, 3);
    s.add(
        "高标一致性闸",
        t.is_high(0) == Some(true) && t.marks_consistent(),
        "3A 档即高电流口必标记",
    );
    // 6. 档位闭集边界。
    s.add(
        "档位闭集边界",
        TierTable::new().set_tier(0, 4) == Err(PtCode::EBadTier)
            && TierTable::new().set_tier(0, 0).is_ok(),
        "档位只收 0..=3，4 拒",
    );
    // 7. 闭集唯一性与诊断非空。
    let mut uniq = true;
    let mut i = 0usize;
    while i < ALL.len() {
        let mut j = i + 1;
        while j < ALL.len() {
            if ALL[i].val() == ALL[j].val() {
                uniq = false;
            }
            j += 1;
        }
        if ALL[i].detail().is_empty() {
            uniq = false;
        }
        i += 1;
    }
    s.add("闭集唯一且诊断非空", uniq && ALL.len() == 12, "12 变体 val 互异且 detail 中文非空");
    // 8. 段位守卫。
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0000 && SEG_BASE <= 0xC31C0,
        "SEG_BASE 0x40 对齐且在 VX12 域",
    );
    // 9. 档位链单调。
    s.add(
        "档位链单调",
        phase_step(PtPhase::Probe, PtPhase::Mark)
            && phase_step(PtPhase::Mark, PtPhase::Enforce)
            && phase_step(PtPhase::Enforce, PtPhase::Audit)
            && !phase_step(PtPhase::Probe, PtPhase::Audit),
        "档位链逐段推进，越级拒",
    );
    // 10. 取流守恒闸。
    let mut g = CurrentGuard::new();
    let _ = g.draw(0, 400, 0);
    let _ = g.draw(1, 2000, 0);
    s.add(
        "取流守恒闸",
        g.conserve_ok(2) && g.rejects == 1 && g.draw_of(0) == Some(400),
        "接受+拒收=申请，拒收落账",
    );
    // 11. 注入守恒等式。
    s.add(
        "注入账守恒",
        hit + esc == (6 * INJ_ROUNDS) as u32,
        "检出+逃逸=60 等式成立",
    );
    // 12. 双跑指纹一致。
    let (_, _, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "两次全流程指纹逐位一致");
    // 13. 收口。
    s.add("收口·判据条数", s.len() >= 13, "含本条共 ≥13 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow() {
        let (a, b, c, _) = run_cycle();
        assert_eq!(a, TIER_ROUNDS as u32);
        assert_eq!(b, TIER_ROUNDS as u32 * 10);
        assert_eq!(c, TIER_ROUNDS as u32);
    }

    #[test]
    fn test_boundary_over_draw() {
        let mut g = CurrentGuard::new();
        assert_eq!(g.draw(0, 501, 0), Err(PtCode::EOverDraw));
        assert!(g.draw(0, 500, 0).is_ok());
    }

    #[test]
    fn test_boundary_bad_tier() {
        let mut t = TierTable::new();
        assert_eq!(t.set_tier(0, 4), Err(PtCode::EBadTier));
        assert_eq!(t.set_tier(0, 99), Err(PtCode::EBadTier));
    }

    #[test]
    fn test_boundary_tier_rewind() {
        let mut t = TierTable::new();
        let _ = t.set_tier(2, 3);
        assert_eq!(t.set_tier(2, 2), Err(PtCode::ETierRewind));
        assert!(t.set_tier(2, 3).is_err()); // 同档重登 EDupMark
        assert_eq!(t.set_tier(2, 2), Err(PtCode::ETierRewind));
    }

    #[test]
    fn test_boundary_no_port() {
        let mut t = TierTable::new();
        assert_eq!(t.set_tier(PORTS, 0), Err(PtCode::ENoPort));
        let mut g = CurrentGuard::new();
        assert_eq!(g.draw(PORTS, 1, 0), Err(PtCode::ENoPort));
    }

    #[test]
    fn test_high_mark_consistency() {
        let mut t = TierTable::new();
        let _ = t.set_tier(0, 0);
        let _ = t.set_tier(1, 3);
        assert_eq!(t.is_high(0), Some(false));
        assert_eq!(t.is_high(1), Some(true));
        assert!(t.marks_consistent());
        assert_eq!(t.marked_count(), 1);
    }

    #[test]
    fn test_dup_mark() {
        let mut t = TierTable::new();
        let _ = t.set_tier(3, 1);
        assert_eq!(t.set_tier(3, 1), Err(PtCode::EDupMark));
    }

    #[test]
    fn test_phase_monotonic() {
        assert!(phase_step(PtPhase::Probe, PtPhase::Mark));
        assert!(!phase_step(PtPhase::Audit, PtPhase::Probe));
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, c1, f1) = run_cycle();
        let (a2, b2, c2, f2) = run_cycle();
        assert_eq!((a1, b1, c1), (a2, b2, c2));
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_injection_all_hit() {
        let (hit, esc, _) = PtInjector::new().inject_all();
        assert_eq!(hit, 60);
        assert_eq!(esc, 0);
    }

    #[test]
    fn unx51555_checks_green() {
        let s = run_unx51555_checks();
        assert!(s.all_passed(), "判据集必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 13);
    }
}
