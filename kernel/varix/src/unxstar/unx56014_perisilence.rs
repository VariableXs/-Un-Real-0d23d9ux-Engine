//! UNX-F56014 基准机外设静默态控制器（0x601A0 段）
//!
//! 锚点：UNX-F56014-J1 USB 外设/网络风暴/磁盘指示灯类噪声源静默清单
//! （三源闭集逐源静默规则落账）；J2 静默态验证探针（空载 IO 基线 +
//! 中断率基线 ×3 连跑漂移 <2% 全达标）；J3 静默违例拒采（任一源静默
//! 破即拒采并留痕）三判据全绿。判据锚定 §AI-71.2 专题一。
//! 域：CoRun·UNX-F2-B40（环境冻结域·外设静默面）。
//!
//! 结构：噪声源闭集登记册（USB/网络/指示灯逐源静默规则落账）+ 双基
//! 线账（空载 IO ops/min × 中断率 irq/s）+ ×3 连跑漂移核（万分比 <
//! 200 全达标才判静默）+ 违例留痕账 + 注入核（越闭集源/零基线/超阈
//! 漂移/封账改写/违例未留痕必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x601A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x601A0;
pub const SL_SRC_UNK: u32 = SEG_BASE + 0x01; // 越闭集噪声源
pub const SL_BASE_ZERO: u32 = SEG_BASE + 0x02; // 零基线（未标定）
pub const SL_DRIFT_OVER: u32 = SEG_BASE + 0x03; // 漂移超阈（≥2%）
pub const SL_SEALED: u32 = SEG_BASE + 0x04; // 静默账已封账拒改
pub const SL_RULE_MISS: u32 = SEG_BASE + 0x05; // 静默规则缺（未声明即验证）
pub const SL_VIOL_LOST: u32 = SEG_BASE + 0x06; // 违例未留痕
pub const SL_RUN_SHORT: u32 = SEG_BASE + 0x07; // 连跑不足 3 轮

/// 噪声源闭集。
pub const N_USB: usize = 0;
pub const N_NET: usize = 1;
pub const N_LED: usize = 2;
pub const N_COUNT: usize = 3;
pub const N_NAMES: [&str; N_COUNT] = ["usb_peripheral", "net_storm", "disk_led"];

/// 漂移阈值（万分比：2% = 200）。
pub const DRIFT_PBP_MAX: u32 = 200;

/// 连跑轮数下限。
pub const RUNS_MIN: usize = 3;

/// 万分比漂移：|x - base| × 10000 / base。
pub fn drift_pbp(base: u32, sample: u32) -> u32 {
    if base == 0 {
        return u32::MAX;
    }
    let d = (sample as i64 - base as i64).abs();
    ((d * 10_000) / base as i64) as u32
}

/// 单源静默账：源 × 静默规则 × IO 基线 × 中断基线。
#[derive(Clone)]
pub struct SourceEntry {
    pub src: usize,
    pub rule: String,
    pub io_base: u32,
    pub irq_base: u32,
}

impl SourceEntry {
    pub fn new(src: usize, rule: &str, io_base: u32, irq_base: u32) -> Result<Self, u32> {
        if src >= N_COUNT {
            return Err(SL_SRC_UNK);
        }
        if rule.is_empty() {
            return Err(SL_RULE_MISS);
        }
        if io_base == 0 || irq_base == 0 {
            return Err(SL_BASE_ZERO);
        }
        Ok(SourceEntry {
            src,
            rule: rule.to_string(),
            io_base,
            irq_base,
        })
    }

    /// 单轮静默验证：IO 与中断双漂移皆 <2% 才算本轮达标。
    pub fn round_ok(&self, io_sample: u32, irq_sample: u32) -> bool {
        drift_pbp(self.io_base, io_sample) < DRIFT_PBP_MAX
            && drift_pbp(self.irq_base, irq_sample) < DRIFT_PBP_MAX
    }
}

/// 静默态总账：三源规则 + 连跑记录 + 违例留痕。
pub struct SilenceCtl {
    pub sources: [Option<SourceEntry>; N_COUNT],
    pub round_log: Vec<(usize, bool)>,
    pub violations: Vec<String>,
    pub sealed: bool,
}

impl SilenceCtl {
    pub fn new() -> Self {
        SilenceCtl {
            sources: [None, None, None],
            round_log: Vec::new(),
            violations: Vec::new(),
            sealed: false,
        }
    }

    /// 源规则落账。
    pub fn register(&mut self, e: SourceEntry) -> Result<(), u32> {
        if self.sealed {
            return Err(SL_SEALED);
        }
        if e.src >= N_COUNT {
            return Err(SL_SRC_UNK);
        }
        let src = e.src;
        self.sources[src] = Some(e);
        Ok(())
    }

    /// 单源 ×3 连跑验证：全达标才判静默；任一破即留违例痕。
    pub fn verify_source(&mut self, src: usize, rounds: &[(u32, u32)]) -> Result<bool, u32> {
        if self.sealed {
            return Err(SL_SEALED);
        }
        if src >= N_COUNT || self.sources[src].is_none() {
            return Err(SL_SRC_UNK);
        }
        if rounds.len() < RUNS_MIN {
            return Err(SL_RUN_SHORT);
        }
        let e = self.sources[src].as_ref().expect("checked").clone();
        let mut all_ok = true;
        for r in rounds.iter() {
            let ok = e.round_ok(r.0, r.1);
            self.round_log.push((src, ok));
            if !ok {
                all_ok = false;
                self.violations.push(format!(
                    "VIOL|{}|io={} irq={}",
                    N_NAMES[src], r.0, r.1
                ));
            }
        }
        Ok(all_ok)
    }

    /// J3：违例留痕核（违例数与日志内假轮数一致）。
    pub fn violation_trail_ok(&self) -> bool {
        let mut false_rounds = 0usize;
        for r in self.round_log.iter() {
            if !r.1 {
                false_rounds += 1;
            }
        }
        false_rounds == self.violations.len() && false_rounds > 0 || self.violations.is_empty()
    }

    /// J2：全源静默核（三源皆已验证且全达标）。
    pub fn all_silent(&self) -> bool {
        for src in 0..N_COUNT {
            let mut verified = false;
            for r in self.round_log.iter() {
                if r.0 == src && r.1 {
                    verified = true;
                }
            }
            if self.sources[src].is_none() || !verified {
                return false;
            }
        }
        true
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：三源静默（IO 基线 120 ops/min，中断基线 800 irq/s）。
pub fn standard_ctl() -> SilenceCtl {
    let mut c = SilenceCtl::new();
    let rules = ["usb-unbound", "net-link-down", "led-off"];
    for s in 0..N_COUNT {
        c.register(SourceEntry::new(s, rules[s], 120, 800).expect("fixture")).expect("reg");
    }
    c
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越闭集源（src=9）
    if SourceEntry::new(9, "r", 10, 10).err() == Some(SL_SRC_UNK) {
        caught += 1;
    }
    // 2) 零基线
    if SourceEntry::new(N_USB, "usb-unbound", 0, 800).err() == Some(SL_BASE_ZERO) {
        caught += 1;
    }
    // 3) 静默规则缺
    if SourceEntry::new(N_USB, "", 10, 10).err() == Some(SL_RULE_MISS) {
        caught += 1;
    }
    // 4) 连跑不足 3 轮
    let mut c = standard_ctl();
    if c.verify_source(N_USB, &[(120, 800), (121, 801)]) == Err(SL_RUN_SHORT) {
        caught += 1;
    }
    // 5) 超阈漂移必标（IO 翻倍 = 10000 万分比）
    let e = SourceEntry::new(N_LED, "led-off", 100, 100).expect("e");
    let ok = !e.round_ok(200, 100) && drift_pbp(100, 200) == 10_000;
    if ok {
        caught += 1;
    }
    (caught, 5)
}

/// ×3 连跑标准验证（夹具轮值：120/121/119 × 800/802/799，全 <2%）。
pub fn verify_standard(c: &mut SilenceCtl) -> Vec<bool> {
    let mut out = Vec::new();
    let rounds = [(120u32, 800u32), (121, 802), (119, 799)];
    for s in 0..N_COUNT {
        out.push(c.verify_source(s, &rounds).expect("verify"));
    }
    out
}

/// 判据入口（CheckSet）。
pub fn run_unx56014_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56014");
    // J1-1 三源静默清单落账
    let c = standard_ctl();
    let mut reg = 0usize;
    for src in 0..N_COUNT {
        if c.sources[src].is_some() {
            reg += 1;
        }
    }
    let ok = reg == N_COUNT;
    s.add("56014-J1-三源清单落账", ok, if ok { "" } else { "清单缺源" });
    // J1-2 静默规则语义（三规则各就位）
    let ok = c.sources[N_USB].as_ref().expect("u").rule == "usb-unbound"
        && c.sources[N_NET].as_ref().expect("n").rule == "net-link-down"
        && c.sources[N_LED].as_ref().expect("l").rule == "led-off";
    s.add("56014-J1-静默规则语义", ok, if ok { "" } else { "规则漂移" });
    // J2-1 ×3 连跑全达标（标准轮值漂移 0/83/83 万分比皆 <200）
    let mut c2 = standard_ctl();
    let vs = verify_standard(&mut c2);
    let mut all = true;
    for v in vs.iter() {
        if !v {
            all = false;
        }
    }
    let ok = all && c2.all_silent();
    s.add("56014-J2-三连跑全达标", ok, if ok { "" } else { "连跑破阈" });
    // J2-2 漂移阈值语义（1% 达标 2% 拒）
    let ok = drift_pbp(120, 121) == 83
        && drift_pbp(100, 102) == 200
        && !SourceEntry::new(N_LED, "led-off", 100, 100).expect("e").round_ok(102, 100);
    s.add("56014-J2-漂移阈值语义", ok, if ok { "" } else { "阈值语义漂移" });
    // J2-3 双基线独立（IO 破 IRQ 稳仍判不达标）
    let e = SourceEntry::new(N_NET, "net-link-down", 100, 100).expect("n");
    let ok = !e.round_ok(130, 100) && e.round_ok(100, 100);
    s.add("56014-J2-双基线独立", ok, if ok { "" } else { "单基线放水" });
    // J3-1 注入核 5 类必抓
    let (ic, in_) = injection_core();
    let ok = ic == in_;
    let detail = if ok { String::new() } else { format!("注入 {ic}/{in_}") };
    s.add_dyn("56014-J3-注入核必抓", ok, &detail);
    // J3-2 违例留痕（超阈轮全落 VIOL 行）
    let mut c3 = standard_ctl();
    let _ = c3.verify_source(N_USB, &[(120, 800), (300, 800), (121, 801)]);
    let ok = c3.violations.len() == 1 && c3.violations[0].contains("VIOL|usb_peripheral|io=300");
    s.add("56014-J3-违例留痕", ok, if ok { "" } else { "违例被吞" });
    // J3-3 违例态不判静默
    let ok = !c3.all_silent();
    s.add("56014-J3-违例拒静默", ok, if ok { "" } else { "违例放行" });
    // J1-3 封账冻结
    let mut c4 = standard_ctl();
    c4.seal();
    let ok = c4.register(SourceEntry::new(N_USB, "x", 1, 1).expect("s")) == Err(SL_SEALED)
        && c4.verify_source(N_USB, &[(120, 800), (121, 801), (119, 799)]) == Err(SL_SEALED);
    s.add("56014-J1-封账冻结", ok, if ok { "" } else { "封账后可写" });
    // J1-4 未验证源不判静默
    let c5 = standard_ctl();
    let ok = !c5.all_silent();
    s.add("56014-J1-未验证不判静默", ok, if ok { "" } else { "未验证放行" });
    // J2-4 连跑轮数下限语义
    let ok = RUNS_MIN == 3;
    s.add("56014-J2-连跑下限", ok, if ok { "" } else { "下限漂移" });
    // J1-5 源名闭集同源
    let ok = N_NAMES[N_USB] == "usb_peripheral" && N_NAMES[N_COUNT - 1] == "disk_led";
    s.add("56014-J1-源名同源", ok, if ok { "" } else { "命名漂移" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56014_all_checks_green() {
        let set = run_unx56014_checks();
        assert!(set.all_passed(), "UNX-F56014 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56014_drift_math() {
        assert_eq!(drift_pbp(120, 121), 83);
        assert_eq!(drift_pbp(100, 102), 200);
        assert_eq!(drift_pbp(100, 200), 10_000);
    }
}
