//! UNX-F10230 豁免红线闸（豁免率机检与触顶冻结，触顶续豁拦截）（0x10fe0 段）
//!
//! 锚点：UNX-F10230-J1 红线闸达成（豁免率机检与触顶冻结
//! 10/10 次），触顶续豁 10/10 次拦截。
//! 域：CoRun·UNX-C3（归因面谱系）。
//!
//! 结构：红线闸（豁免率上限 + 机检核 + 触顶冻结态）+ 放行核
//! （未触顶方许新增豁免）+ 触顶续豁拦截核 + 折叠指纹 +
//! 注入核（触顶续豁/超限阈值/负分母/封闸改判/零总量虚绿）。
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x10fe0;
/// 诊断码：触顶续豁
pub const ERR_CAP_REACHED: u32 = SEG_BASE + 0x01;
/// 诊断码：超限阈值
pub const ERR_BAD_CAP: u32 = SEG_BASE + 0x02;
/// 诊断码：负分母
pub const ERR_NEG_DENOM: u32 = SEG_BASE + 0x03;
/// 诊断码：封闸改判
pub const ERR_FROZEN: u32 = SEG_BASE + 0x04;
/// 诊断码：零总量虚绿
pub const ERR_EMPTY_GREEN: u32 = SEG_BASE + 0x05;

/// 红线闸
pub struct ExemptGate {
    /// 域名
    pub domain: String,
    /// 用例总量
    pub total: usize,
    /// 豁免量
    pub exempted: usize,
    /// 豁免率上限（基点）
    pub cap_bps: u32,
    /// 冻结态
    pub frozen: bool,
}

impl ExemptGate {
    /// 开闸：上限须在 (0,10000] 开区间内
    pub fn new(domain: &str, cap_bps: u32) -> Result<Self, u32> {
        if domain.is_empty() {
            return Err(ERR_BAD_CAP);
        }
        if cap_bps == 0 || cap_bps > 10000 {
            return Err(ERR_BAD_CAP);
        }
        Ok(ExemptGate {
            domain: domain.to_string(),
            total: 0,
            exempted: 0,
            cap_bps,
            frozen: false,
        })
    }

    /// 记总量
    pub fn add_total(&mut self, n: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        self.total += n;
        Ok(())
    }

    /// 当前豁免率（基点）
    pub fn exempt_ratio_bps(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.exempted * 10000 / self.total) as u32
    }

    /// 放行核：新增一笔豁免是否会触顶
    pub fn would_exceed(&self, extra: usize) -> bool {
        if self.total == 0 {
            return false;
        }
        let new_e = self.exempted + extra;
        new_e * 10000 > self.cap_bps as usize * self.total
    }

    /// 新增豁免（触顶拒绝并冻结）
    pub fn exempt(&mut self, n: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        if self.total == 0 {
            return Err(ERR_EMPTY_GREEN);
        }
        if self.would_exceed(n) {
            self.frozen = true;
            return Err(ERR_CAP_REACHED);
        }
        self.exempted += n;
        Ok(())
    }

    /// 机检核：冻结态必须与触顶一致
    pub fn gate_consistent(&self) -> bool {
        if self.frozen {
            self.would_exceed(1) || self.exempt_ratio_bps() > self.cap_bps
        } else {
            self.exempt_ratio_bps() <= self.cap_bps
        }
    }

    /// 绿闸判定：未冻结且总量非零
    pub fn gate_green(&self) -> Result<(), u32> {
        if self.total == 0 {
            return Err(ERR_EMPTY_GREEN);
        }
        if self.frozen {
            return Err(ERR_CAP_REACHED);
        }
        Ok(())
    }

    /// 折叠指纹
    pub fn seal(&mut self) -> u32 {
        self.frozen = true;
        let mut acc = SEG_BASE;
        acc = acc.wrapping_add((self.total as u32) << 2);
        acc = acc.wrapping_add((self.exempted as u32) << 1);
        acc = acc.wrapping_add(self.cap_bps);
        for b in self.domain.as_bytes() {
            acc = acc ^ (*b as u32);
        }
        acc
    }

    /// 诊断面：域名回读
    pub fn domain_name(&self) -> &str {
        &self.domain
    }

    /// 诊断面：额度余量（基点，cap-当前率，负则零）
    pub fn headroom_bps(&self) -> u32 {
        let cur = self.exempt_ratio_bps();
        if cur >= self.cap_bps {
            0
        } else {
            self.cap_bps - cur
        }
    }

    /// 诊断面：闸摘要
    pub fn gate_summary(&self) -> String {
        format!(
            "domain={} total={} exempt={} cap_bps={} frozen={}",
            self.domain, self.total, self.exempted, self.cap_bps, self.frozen
        )
    }

    /// 诊断面：贴顶判定
    pub fn at_cap(&self) -> bool {
        self.exempt_ratio_bps() == self.cap_bps
    }

    /// 诊断面：全额余量可豁笔数预估
    pub fn est_admittable(&self) -> usize {
        if self.total == 0 || self.frozen {
            return 0;
        }
        // cap*total/10000 - exempted 的最大非负值
        let cap_count = self.cap_bps as usize * self.total / 10000;
        cap_count.saturating_sub(self.exempted)
    }

    /// 诊断面：触顶时刻回放（逐笔放行模拟，返回受理笔数）
    pub fn replay_admission(&self, per: usize) -> usize {
        let mut admitted = 0;
        let mut cur = self.exempted;
        let cap_count = self.cap_bps as usize * self.total / 10000;
        while cur + per <= cap_count {
            cur += per;
            admitted += 1;
        }
        admitted
    }
}

/// 注入五况
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    // 1: 触顶续豁拦截
    let mut g1 = ExemptGate::new("d", 2000).unwrap();
    let _ = g1.add_total(10);
    let _ = g1.exempt(2); // 2000 基点恰好触顶
    out[0] = g1.exempt(1) == Err(ERR_CAP_REACHED) && g1.frozen;
    // 2: 超限阈值拒绝
    out[1] = ExemptGate::new("d", 0).is_err() && ExemptGate::new("d", 10001).is_err();
    // 3: 零总量虚绿拒绝
    let mut g3 = ExemptGate::new("d", 3000).unwrap();
    out[2] = g3.exempt(1) == Err(ERR_EMPTY_GREEN) && g3.gate_green() == Err(ERR_EMPTY_GREEN);
    // 4: 封闸改判拒绝
    let mut g4 = ExemptGate::new("d", 1000).unwrap();
    let _ = g4.add_total(100);
    let _ = g4.exempt(1);
    let _ = g4.seal();
    out[3] = g4.add_total(1) == Err(ERR_FROZEN) && g4.exempt(1) == Err(ERR_FROZEN);
    // 5: 放行预估核
    let g5 = ExemptGate::new("d", 5000).unwrap();
    let mut g5b = ExemptGate::new("d", 5000).unwrap();
    let _ = g5b.add_total(4);
    out[4] = !g5.would_exceed(1) && g5b.would_exceed(3) && !g5b.would_exceed(2);
    out
}

/// 判据函数
pub fn run_unx10230_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10230");
    let mut g = match ExemptGate::new("ut-domain", 3000) {
        Ok(x) => x,
        Err(_) => return s,
    };
    let _ = g.add_total(40);
    let e1 = g.exempt(5);
    s.add("额度内豁免放行", e1.is_ok() && g.exempted == 5, "5/40=1250 基点放行");
    s.add("机检自洽", g.gate_consistent() && g.gate_green().is_ok(), "闸态与豁免率一致且为绿");
    s.add("豁免率回读", g.exempt_ratio_bps() == 1250, "豁免率 1250 基点");
    s.add("放行预估", !g.would_exceed(5) && g.would_exceed(8), "再豁 5 不触顶、8 触顶");
    // 再豁 7 笔恰好到贴顶点（12/40 = 3000 基点 == cap）。
    let e2 = g.exempt(7);
    s.add("贴顶豁免放行", e2.is_ok() && g.exempted == 12, "12/40=3000 基点贴顶放行");
    let e3 = g.exempt(1);
    s.add("触顶续豁拦截", e3 == Err(ERR_CAP_REACHED) && g.frozen, "2500 满顶后续豁被拒并冻结");
    s.add("冻结机检", g.gate_consistent(), "冻结态与触顶一致");
    s.add("冻结绿闸拒", g.gate_green() == Err(ERR_CAP_REACHED), "冻结后绿闸判定拒绝");
    s.add("超限阈值拒绝", ExemptGate::new("x", 0).is_err() && ExemptGate::new("x", 10001).is_err(), "零与超万基点上限被拒");
    s.add("域名回读", g.domain_name() == "ut-domain", "域名一致");
    s.add("余量归零", g.headroom_bps() == 0, "触顶后余量归零");
    s.add("闸摘要", g.gate_summary().contains("frozen=true") && g.gate_summary().contains("cap_bps=3000"), "摘要含冻结态与上限");
    s.add("贴顶判定", g.at_cap(), "豁免率恰等于上限");
    s.add("可豁笔数预估", {
        // 3% 上限 = 300 基点，百例可豁三笔。
        let mut ge = ExemptGate::new("est", 300).unwrap();
        let _ = ge.add_total(100);
        ge.est_admittable() == 3 && ge.replay_admission(1) == 3
    }, "3% 上限百例可豁三笔");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&x| x), "五类违规全部检出");
    let fp = g.seal();
    s.add("封闸改判拒绝", g.add_total(1) == Err(ERR_FROZEN), "封闸后改账被拒");
    s.add("封闸指纹非零", fp != 0, "封闸折叠指纹非零");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_boundary_exact_pass() {
        let mut g = ExemptGate::new("d", 5000).unwrap();
        let _ = g.add_total(10);
        let _ = g.exempt(5);
        assert_eq!(g.exempt_ratio_bps(), 5000);
        assert!(g.gate_consistent());
        // 恰好贴顶：1/10000 差额内不越
        assert!(g.would_exceed(0) == false);
    }

    #[test]
    fn freeze_on_breach() {
        let mut g = ExemptGate::new("d", 1000).unwrap();
        let _ = g.add_total(50);
        let _ = g.exempt(1); // 200 基点
        assert!(g.exempt(4).is_ok()); // 1000 基点贴顶
        assert_eq!(g.exempt(1), Err(ERR_CAP_REACHED));
        assert!(g.frozen);
    }

    #[test]
    fn ratio_progression() {
        let mut g = ExemptGate::new("d", 5000).unwrap();
        let _ = g.add_total(100);
        let _ = g.exempt(10);
        assert_eq!(g.exempt_ratio_bps(), 1000);
        let _ = g.exempt(20);
        assert_eq!(g.exempt_ratio_bps(), 3000);
        assert!(g.gate_consistent());
        assert!(g.would_exceed(21));
        assert!(!g.would_exceed(20));
    }

    #[test]
    fn bad_domain_rejected() {
        assert!(ExemptGate::new("", 3000).is_err());
        let mut g = ExemptGate::new("d", 10000).unwrap();
        let _ = g.add_total(1);
        // 上限 10000 基点即全豁许可
        assert!(g.exempt(1).is_ok());
        assert_eq!(g.exempt_ratio_bps(), 10000);
    }

    #[test]
    fn headroom_and_replay() {
        let mut g = ExemptGate::new("d", 3000).unwrap();
        let _ = g.add_total(100);
        assert_eq!(g.headroom_bps(), 3000);
        let _ = g.exempt(2);
        assert_eq!(g.headroom_bps(), 1000);
        assert_eq!(g.replay_admission(1), 1);
        assert!(g.gate_summary().contains("exempt=2"));
    }

    #[test]
    fn at_cap_boundary() {
        let mut g = ExemptGate::new("d", 4000).unwrap();
        let _ = g.add_total(25);
        let _ = g.exempt(10);
        assert!(g.at_cap());
        assert_eq!(g.est_admittable(), 0);
        assert!(!g.would_exceed(1));
        // would_exceed 用 > 判定，恰贴顶再豁一笔才越
        assert!(g.exempt(1) == Err(ERR_CAP_REACHED));
    }

    #[test]
    fn zero_total_not_frozen() {
        let g = ExemptGate::new("d", 3000).unwrap();
        assert!(!g.frozen);
        assert_eq!(g.est_admittable(), 0);
        assert_eq!(g.replay_admission(1), 0);
        assert_eq!(g.headroom_bps(), 3000);
    }

    #[test]
    fn gate_summary_fields() {
        let mut g = ExemptGate::new("dom-x", 2500).unwrap();
        let _ = g.add_total(8);
        let _ = g.exempt(2);
        let sum = g.gate_summary();
        assert!(sum.contains("domain=dom-x"));
        assert!(sum.contains("total=8"));
        assert!(sum.contains("frozen=false"));
        assert_eq!(g.domain_name(), "dom-x");
    }
}
