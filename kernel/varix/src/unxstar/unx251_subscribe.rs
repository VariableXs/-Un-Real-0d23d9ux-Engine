//! UNX-F0251 早传第十步：消费方订阅接口（域间早传分发）（0x3CC0 段）
//!
//! 锚点：UNX-F0251-J1 A2/A3/A4/A5 订阅声明注册，各域取所需节
//! （引用不拷贝），越界取节拒绝。
//! 域：CoRun·UNX-A1-B13（订阅分发面）。
//!
//! 结构：消费方域闭集（A2 内存 / A3 CPU / A4 驱动 / A5 报告）+
//! 订阅声明（域 × 节集，节引用十二步 Manifest 节号）+ 注册器
//! （声明入账，重复节同域幂等）+ 取节核（引用语义：只借不拷，
//! 返回节号切片引用）+ 越界核（未订阅节 / 节号越出十二步界必拒）+
//! 注入核（未注册域 / 未订阅节 / 越界节 / 封账拒写必抓）。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;

/// 诊断段（0x3CC0 独占 0x10）。
pub const SEG_BASE: u32 = 0x3CC0;
pub const SB_DOM_OOB: u32 = SEG_BASE + 0x01; // 消费方域越集
pub const SB_STEP_OOB: u32 = SEG_BASE + 0x02; // 节号越出十二步界
pub const SB_NOT_SUB: u32 = SEG_BASE + 0x03; // 未订阅取节
pub const SB_SEALED: u32 = SEG_BASE + 0x04; // 注册器已封账拒写
pub const SB_EMPTY_SUB: u32 = SEG_BASE + 0x05; // 空订阅声明

/// 消费方域闭集：A2 内存 / A3 CPU / A4 驱动 / A5 报告。
pub const DOM_A2: usize = 0;
pub const DOM_A3: usize = 1;
pub const DOM_A4: usize = 2;
pub const DOM_A5: usize = 3;
pub const DOM_COUNT: usize = 4;

/// 域名。
pub const DOM_NAMES: [&str; DOM_COUNT] = ["A2内存", "A3CPU", "A4驱动", "A5报告"];

/// 十二步节号界（1..=12，与 F0241 十二步总纲同源）。
pub const STEP_FIRST: u32 = 1;
pub const STEP_LAST: u32 = 12;

/// 节名（十二步，F0241 名册同源）。
pub const STEP_NAMES: [&str; 12] = [
    "固件", "内存", "CPU", "随机化", "参数", "健康", "时间", "日志", "上次引导", "订阅", "完整性", "时序",
];

/// 节号合法核。
pub fn step_in_range(step: u32) -> bool {
    step >= STEP_FIRST && step <= STEP_LAST
}

/// 订阅声明：域 + 节集（升序去重由注册器保证）。
#[derive(Clone)]
pub struct Subscription {
    pub dom: usize,
    pub steps: Vec<u32>,
}

/// 订阅注册器（域间早传分发枢纽）。
pub struct SubscribeHub {
    pub subs: Vec<Subscription>,
    pub sealed: bool,
}

impl SubscribeHub {
    pub fn new() -> Self {
        SubscribeHub { subs: Vec::new(), sealed: false }
    }

    /// 声明订阅（域闭集 + 节号界守卫；空订阅拒；同域同节幂等）。
    pub fn declare(&mut self, dom: usize, steps: &[u32]) -> Result<(), u32> {
        if self.sealed {
            return Err(SB_SEALED);
        }
        if dom >= DOM_COUNT {
            return Err(SB_DOM_OOB);
        }
        if steps.is_empty() {
            return Err(SB_EMPTY_SUB);
        }
        for &st in steps {
            if !step_in_range(st) {
                return Err(SB_STEP_OOB);
            }
        }
        // 幂等合并：已有声明则并集，否则新声明
        if let Some(sub) = self.subs.iter_mut().find(|s| s.dom == dom) {
            for &st in steps {
                if !sub.steps.contains(&st) {
                    sub.steps.push(st);
                }
            }
            sub.steps.sort_unstable();
        } else {
            let mut v = Vec::new();
            for &st in steps {
                if !v.contains(&st) {
                    v.push(st);
                }
            }
            v.sort_unstable();
            self.subs.push(Subscription { dom, steps: v });
        }
        Ok(())
    }

    /// 取节核（引用不拷贝）：返回该域订阅节集的只读引用。
    /// 未注册域或未订阅节一律拒绝。
    pub fn take_ref(&self, dom: usize) -> Result<&[u32], u32> {
        if dom >= DOM_COUNT {
            return Err(SB_DOM_OOB);
        }
        match self.subs.iter().find(|s| s.dom == dom) {
            Some(s) => Ok(&s.steps),
            None => Err(SB_NOT_SUB),
        }
    }

    /// 单节访问核：域已订阅该节才放行。
    pub fn access(&self, dom: usize, step: u32) -> Result<(), u32> {
        if !step_in_range(step) {
            return Err(SB_STEP_OOB);
        }
        let sub = self.take_ref(dom)?;
        if sub.contains(&step) {
            Ok(())
        } else {
            Err(SB_NOT_SUB)
        }
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 分发矩阵核：每域订阅节号全部合法且升序。
    pub fn matrix_valid(&self) -> bool {
        self.subs.iter().all(|s| {
            s.dom < DOM_COUNT
                && !s.steps.is_empty()
                && s.steps.windows(2).all(|w| w[0] < w[1])
                && s.steps.iter().all(|&st| step_in_range(st))
        })
    }

    /// Manifest 渲染（分发矩阵行）。
    pub fn render(&self) -> String {
        let mut out = format!("订阅分发 注册域={}\n", self.subs.len());
        for s in &self.subs {
            let names: Vec<&str> = s.steps.iter().map(|&st| STEP_NAMES[(st - 1) as usize]).collect();
            let line = format!("  {} 订阅节 {:?} 名 {:?}\n", DOM_NAMES[s.dom], s.steps, names);
            out.push_str(&line);
        }
        out
    }
}

/// 标准夹具：四域各订阅所需节（A2 内存/CPU 面，A3 CPU/健康面，
/// A4 固件/内存/参数面，A5 全景报告面四节）。
pub fn standard_hub() -> SubscribeHub {
    let mut h = SubscribeHub::new();
    let _ = h.declare(DOM_A2, &[2, 3]); // 内存节 + CPU 节
    let _ = h.declare(DOM_A3, &[3, 6]); // CPU 节 + 健康节
    let _ = h.declare(DOM_A4, &[1, 2, 5]); // 固件 + 内存 + 参数
    let _ = h.declare(DOM_A5, &[6, 7, 9, 12]); // 健康 + 时间 + 上次引导 + 时序
    h
}

/// 注入核：7 注入必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut h = SubscribeHub::new();
    // 1) 域越集
    if h.declare(9, &[1]) == Err(SB_DOM_OOB) {
        caught += 1;
    }
    // 2) 节号越界（0 与 13 两向）
    if h.declare(DOM_A2, &[0]) == Err(SB_STEP_OOB) {
        caught += 1;
    }
    if h.declare(DOM_A2, &[13]) == Err(SB_STEP_OOB) {
        caught += 1;
    }
    // 3) 空订阅拒
    if h.declare(DOM_A2, &[]) == Err(SB_EMPTY_SUB) {
        caught += 1;
    }
    // 4) 未注册域取节拒
    if h.take_ref(DOM_A3) == Err(SB_NOT_SUB) {
        caught += 1;
    }
    // 5) 未订阅节访问拒
    let _ = h.declare(DOM_A2, &[2]);
    if h.access(DOM_A2, 12) == Err(SB_NOT_SUB) {
        caught += 1;
    }
    // 6) 封账拒写
    h.seal();
    if h.declare(DOM_A3, &[1]) == Err(SB_SEALED) {
        caught += 1;
    }
    // 7) 幂等重声明不重复（同域同节再声明后节集不增）
    let mut h2 = standard_hub();
    let before = h2.subs.iter().find(|s| s.dom == DOM_A2).map(|s| s.steps.len()).unwrap_or(0);
    let _ = h2.declare(DOM_A2, &[2, 3]);
    let after = h2.subs.iter().find(|s| s.dom == DOM_A2).map(|s| s.steps.len()).unwrap_or(0);
    if before == after && before == 2 {
        caught += 1;
    }
    (caught, 7)
}

/// 判据入口（CheckSet）。
pub fn run_unx251_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0251");
    // J1-1 四域订阅声明注册
    let h = standard_hub();
    let ok = h.subs.len() == DOM_COUNT && h.matrix_valid();
    s.add("B13-24-四域声明注册", ok, if ok { "" } else { "注册缺域或矩阵破" });
    // J1-2 引用取节（不拷贝语义：切片引用可用 + 内容与声明一致）
    let h = standard_hub();
    let ok = h.take_ref(DOM_A2) == Ok(&[2u32, 3u32][..])
        && h.take_ref(DOM_A5) == Ok(&[6u32, 7u32, 9u32, 12u32][..]);
    s.add("B13-25-引用取节", ok, if ok { "" } else { "取节漂移" });
    // J1-3 越界取节拒绝（未订阅节 + 越界节号 + 未注册域三向）
    let h = standard_hub();
    let ok = h.access(DOM_A2, 12) == Err(SB_NOT_SUB)
        && h.access(DOM_A2, 0) == Err(SB_STEP_OOB)
        && h.access(DOM_A2, 13) == Err(SB_STEP_OOB);
    s.add("B13-26-越界取节拒", ok, if ok { "" } else { "越界放行" });
    // J1-4 分发渲染（每域行齐 + 节名映射正确）
    let h = standard_hub();
    let r = h.render();
    let ok = r.contains("A2内存") && r.contains("A5报告") && r.contains("健康");
    s.add("B13-27-分发渲染", ok, if ok { "" } else { "渲染缺行" });
    // J1-5 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B13-28-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx251_all_checks_green() {
        let set = run_unx251_checks();
        assert!(set.all_passed(), "UNX-F0251 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx251_ref_not_copy() {
        let h = standard_hub();
        // 引用语义：拿到的切片与注册表同源（改声明即改引用视图）
        let sub = h.subs.iter().find(|s| s.dom == DOM_A4).unwrap();
        assert_eq!(sub.steps.as_slice(), &[1u32, 2u32, 5u32]);
    }
}
