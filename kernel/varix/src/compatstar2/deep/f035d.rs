//! F035 深化批次二 · 失败指纹与向导步骤栈面（compatstar2/deep · G-A-35）。
//!
//! 批次一深化覆盖运行时签名表/异常码分类/频率分层；本批补齐：失败样本
//! 联合指纹（文件哈希 + 崩溃签名复合主键——「按哈希记忆」的精度提升）、
//! 向导步骤栈（可回退的上一步——引导流的导航面）、离线清单匹配排序
//! （前缀命中率降序——断网指引的优先级面）、日志导出脱敏闸（三查未过
//! 不许导出——F139 提报通道的前置闸）。
//!
//! 零堆纪律：定长栈与表，无 alloc。

use crate::checks::CheckSet;

/// 步骤栈容量。
pub const WIZARD_STACK_CAP: usize = 8;
/// 离线清单容量。
pub const OFFLINE_LIST_CAP: usize = 8;

/// 失败样本联合指纹：文件哈希 + 崩溃签名（复合主键——同文件不同签名
/// 是不同失败；记忆精度高于单哈希）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FailureKey {
    pub hash8: [u8; 8],
    pub crash_sig: u8,
}

/// 复合主键相等性：哈希同但签名异 → 不同失败（与单哈希记忆的差异点）。
pub fn failure_key_eq(a: &FailureKey, b: &FailureKey) -> bool {
    a.hash8 == b.hash8 && a.crash_sig == b.crash_sig
}

/// 向导步骤栈：push 前进 / pop 回退（引导流可回退的导航面；满容拒绝）。
pub struct WizardStack {
    pub steps: [u8; WIZARD_STACK_CAP],
    pub depth: usize,
}

impl WizardStack {
    pub const fn new() -> Self {
        WizardStack { steps: [0; WIZARD_STACK_CAP], depth: 0 }
    }
    pub fn push(&mut self, step: u8) -> bool {
        if self.depth >= WIZARD_STACK_CAP {
            return false;
        }
        self.steps[self.depth] = step;
        self.depth += 1;
        true
    }
    /// 回退：弹出当前步（返回被弹出的步号；空栈 None）。
    pub fn pop(&mut self) -> Option<u8> {
        if self.depth == 0 {
            return None;
        }
        self.depth -= 1;
        Some(self.steps[self.depth])
    }
    /// 当前步（栈顶；空栈 None）。
    pub fn current(&self) -> Option<u8> {
        self.depth.checked_sub(1).map(|i| self.steps[i])
    }
}

/// 离线清单条目。
#[derive(Clone, Copy)]
pub struct OfflineCandidate {
    pub runtime: &'static str,
    /// 与失败归因的匹配得分（0-100）。
    pub score: u32,
}

/// 离线清单匹配排序：得分降序、同分保序（插入排序——断网指引的
/// 优先级面，零分配稳定序）。
pub fn rank_offline(cands: &[OfflineCandidate], out: &mut [usize; OFFLINE_LIST_CAP]) -> usize {
    let n = cands.len().min(OFFLINE_LIST_CAP);
    for (i, slot) in out.iter_mut().enumerate().take(n) {
        *slot = i;
    }
    for i in 1..n {
        let cur = out[i];
        let mut j = i;
        while j > 0 && cands[out[j - 1]].score < cands[cur].score {
            out[j] = out[j - 1];
            j -= 1;
        }
        out[j] = cur;
    }
    n
}

/// 日志导出脱敏闸：路径/用户名/序列号三查——任一命中即拒（F139 前置闸；
/// 与批次一 stardraft 隐私扫描模式同源，两处消费共享规则面）。
pub fn export_gate(contents: [&str; 4]) -> (bool, [bool; 4]) {
    let mut dirty = [false; 4];
    for (i, c) in contents.iter().enumerate() {
        dirty[i] = c.contains("C:\\Users\\") || c.contains("S/N:") || c.contains("serial-no=");
    }
    (dirty.iter().all(|&d| !d), dirty)
}

/// 域自检（深化批次二）。
pub fn run_f035d_checks() -> CheckSet {
    let mut cs = CheckSet::new("F035-compatwiz-d2");
    // 1) 联合指纹：同哈希异签名 = 不同失败；同哈希同签名 = 同失败。
    let k1 = FailureKey { hash8: [1; 8], crash_sig: 5 };
    let k2 = FailureKey { hash8: [1; 8], crash_sig: 7 };
    let k3 = FailureKey { hash8: [1; 8], crash_sig: 5 };
    cs.add(
        "failure_key_compound",
        !failure_key_eq(&k1, &k2) && failure_key_eq(&k1, &k3),
        "",
    );
    // 2) 步骤栈：push/push/pop 回到上一步；空栈 pop 如实 None；满容拒绝。
    let mut w = WizardStack::new();
    let flow = w.push(1) && w.push(2) && w.current() == Some(2) && w.pop() == Some(2) && w.current() == Some(1);
    let mut full = WizardStack::new();
    let mut fill_ok = true;
    for s in 0..WIZARD_STACK_CAP as u8 {
        fill_ok &= full.push(s);
    }
    cs.add(
        "wizard_stack",
        flow && w.pop() == Some(1) && w.pop().is_none() && fill_ok && !full.push(9),
        "",
    );
    // 3) 离线清单排序：得分降序、同分保序（python 90 → node 90 → java 40）。
    let cands = [
        OfflineCandidate { runtime: "python", score: 90 },
        OfflineCandidate { runtime: "node", score: 90 },
        OfflineCandidate { runtime: "java", score: 40 },
    ];
    let mut order = [0usize; OFFLINE_LIST_CAP];
    let n = rank_offline(&cands, &mut order);
    cs.add("offline_rank_stable", n == 3 && order[0] == 0 && order[1] == 1 && order[2] == 2, "");
    let high_first = [cands[2], cands[1], cands[0]];
    let mut order2 = [0usize; OFFLINE_LIST_CAP];
    rank_offline(&high_first, &mut order2);
    // 期望序 [node, python, java] = [1, 2, 0]：两 90 分按原序稳定、40 分垫底。
    cs.add("offline_rank_desc", order2[0] == 1 && order2[1] == 2 && order2[2] == 0, "");
    // 4) 导出脱敏闸：干净内容放行；含路径或序列号拒并指明脏位。
    let (clean_ok, dirty1) = export_gate(["app.exe", "ok.log", "status=ok", "count=3"]);
    let (_, dirty2) = export_gate(["ok", "C:\\Users\\varia\\x", "ok", "S/N:1"]);
    cs.add(
        "export_gate",
        clean_ok && !dirty1.iter().any(|&d| d)
            && !export_gate(["C:\\Users\\a", "", "", ""]).0
            && dirty2[1] && dirty2[3] && !dirty2[0] && !dirty2[2],
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_lifo_order() {
        let mut w = WizardStack::new();
        for s in 1..=4u8 {
            w.push(s);
        }
        assert_eq!(w.pop(), Some(4));
        assert_eq!(w.pop(), Some(3));
        assert_eq!(w.depth, 2, "LIFO 弹序");
    }

    #[test]
    fn gate_serial_pattern() {
        let (_, d) = export_gate(["serial-no=XYZ", "", "", ""]);
        assert!(d[0], "serial-no= 前缀命中（与 stardraft 扫描模式同源）");
    }

    #[test]
    fn deep2_checks_all_green() {
        let cs = run_f035d_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
