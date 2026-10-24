//! cga41_errdisc.rs ↔ CGPU-F0041 CPU 路径错误码纪律（0x1BE0 段）
//!
//! 职责（施工书原文）：
//!   CPU 渲染路径的错误零静默：错误码段（F0017 注册表分配的 A 域 CPU 段）、
//!   五元组上抛纪律（同 VE-F0015）、注入测试（每类错误注入后必达观察点）。
//!   CPU 路径特有风险：SIMD 路径的越界写（F0001 防护）、线程池死锁
//!   （F0003 死锁检测）——错误处理与防护联动的完整性复核。
//!
//! 判据（施工书原文）：
//!   码段分配、五元组、注入达观察点、联动复核、零静默断言。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1BE0 独占 0x10）。
pub const SEG_BASE: u16 = 0x1BE0;
pub const XC_SILENT: u16 = SEG_BASE + 1; // 静默吞错
pub const XC_TUPLE: u16 = SEG_BASE + 2; // 五元组残缺
pub const XC_OOB: u16 = SEG_BASE + 3; // SIMD 越界写
pub const XC_DEADLOCK: u16 = SEG_BASE + 4; // 池死锁

/// 五元组（同 VE-F0015 口径）：码/域/位置/成因/建议
#[derive(Clone, PartialEq, Debug)]
pub struct ErrTuple {
    pub code: u16,
    pub domain: &'static str,
    pub site: &'static str,
    pub cause: String,
    pub remedy: String,
}

impl ErrTuple {
    pub fn new(code: u16, domain: &'static str, site: &'static str, cause: &str, remedy: &str) -> Self {
        ErrTuple {
            code,
            domain,
            site,
            cause: cause.to_string(),
            remedy: remedy.to_string(),
        }
    }
    /// 五元组完整性（判据 XT 校验口径）：五字段全非空
    pub fn complete(&self) -> bool {
        self.code >= SEG_BASE
            && self.code < SEG_BASE + 0x10
            && !self.domain.is_empty()
            && !self.site.is_empty()
            && !self.cause.is_empty()
            && !self.remedy.is_empty()
    }
}

/// 错误码段注册表（F0017 分配的 A 域 CPU 段内子码闭集）
pub const SEG_TABLE: [(&str, u16); 4] = [
    ("SIMD越界", XC_OOB),
    ("池死锁", XC_DEADLOCK),
    ("静默吞错", XC_SILENT),
    ("五元组残缺", XC_TUPLE),
];

/// 观察点（注入测试的必达终点）
#[derive(Default)]
pub struct WatchPoint {
    pub hits: Vec<u16>, // 到达的码序列
}

impl WatchPoint {
    pub fn hit(&mut self, code: u16) { self.hits.push(code); }
    pub fn reached(&self, code: u16) -> bool { self.hits.contains(&code) }
    /// 零静默断言：每个注入码都必达
    pub fn all_reached(&self, injected: &[u16]) -> bool {
        injected.iter().all(|&c| self.reached(c))
    }
}

/// 上抛纪律：Err 必须带完整五元组才能出域（残缺 = XC_TUPLE 拒收）
pub fn escalate(t: &ErrTuple, wp: &mut WatchPoint) -> Result<(), u16> {
    if !t.complete() {
        let bad = ErrTuple::new(XC_TUPLE, "CGPU-A", "cga41::escalate", "五元组残缺", "补全码/域/位置/成因/建议");
        wp.hit(XC_TUPLE);
        return Err(bad.code);
    }
    wp.hit(t.code);
    Ok(())
}

/// SIMD 越界写防护联动（F0001）：批写前做长度闸
pub fn simd_batch_write_guard(dst_len: usize, offset: usize, count: usize) -> Result<(), u16> {
    if offset.checked_add(count).map(|end| end > dst_len).unwrap_or(true) {
        return Err(XC_OOB);
    }
    Ok(())
}

/// 线程池死锁检测联动（F0003）：任务依赖闭环检测（ Kahn 简化版）
pub fn pool_cycle_check(deps: &[(u32, u32)], n: u32) -> Result<(), u16> {
    // deps: (task, depends_on)；检测闭环
    let mut indeg = vec![0u32; n as usize];
    let mut succ: Vec<Vec<u32>> = vec![Vec::new(); n as usize];
    for &(t, d) in deps {
        if t >= n || d >= n {
            return Err(XC_DEADLOCK);
        }
        succ[d as usize].push(t);
        indeg[t as usize] += 1;
    }
    let mut queue: Vec<u32> = (0..n).filter(|&i| indeg[i as usize] == 0).collect();
    let mut done = 0u32;
    while let Some(i) = queue.pop() {
        done += 1;
        for &s in &succ[i as usize] {
            indeg[s as usize] -= 1;
            if indeg[s as usize] == 0 {
                queue.push(s);
            }
        }
    }
    if done != n {
        return Err(XC_DEADLOCK); // 闭环 = 死锁
    }
    Ok(())
}

/// 注入测试台：每类错误注入 → 上抛 → 必达观察点
pub fn injection_suite() -> (Vec<u16>, WatchPoint) {
    let mut wp = WatchPoint::default();
    let mut injected = Vec::new();
    // 注入 1：SIMD 越界
    let r = simd_batch_write_guard(16, 8, 16);
    if r == Err(XC_OOB) {
        injected.push(XC_OOB);
        let t = ErrTuple::new(XC_OOB, "CGPU-A", "cga41::simd_guard", "批写越界", "钳位 offset+count ≤ len");
        let _ = escalate(&t, &mut wp);
    }
    // 注入 2：池死锁（依赖环 0→1→0）
    let r = pool_cycle_check(&[(1, 0), (0, 1)], 2);
    if r == Err(XC_DEADLOCK) {
        injected.push(XC_DEADLOCK);
        let t = ErrTuple::new(XC_DEADLOCK, "CGPU-A", "cga41::pool_cycle", "依赖闭环", "拆环或拒绝入池");
        let _ = escalate(&t, &mut wp);
    }
    // 注入 3：静默吞错类——源错误 XC_SILENT 以完整五元组上抛（必达）
    let silent = ErrTuple::new(XC_SILENT, "CGPU-A", "cga41::silent", "错误被静默吞没", "任何 Err 必须 escalate 留账");
    if escalate(&silent, &mut wp).is_ok() {
        injected.push(XC_SILENT);
    }
    // 注入 4：五元组残缺（残缺拒收码 XC_TUPLE 必达）
    let bad = ErrTuple::new(XC_SILENT, "CGPU-A", "cga41::silent", "", "任何");
    if escalate(&bad, &mut wp) == Err(XC_TUPLE) {
        injected.push(XC_TUPLE);
    }
    (injected, wp)
}

pub fn self_audit() -> Result<(), u16> {
    let (injected, wp) = injection_suite();
    if !wp.all_reached(&injected) {
        return Err(XC_SILENT);
    }
    Ok(())
}

pub fn run_cga41_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0041");
    // J1 码段分配（注册表 4 行全在段内 + 互异 + 对外常量一致）
    let ok = SEG_TABLE.len() == 4
        && SEG_TABLE.iter().all(|&(_, c)| c >= SEG_BASE && c < SEG_BASE + 0x10)
        && SEG_TABLE.iter().map(|&(_, c)| c).collect::<Vec<_>>().windows(2).all(|w| w[0] != w[1])
        && SEG_BASE % 0x10 == 0;
    s.add("CGA-41-01-码段分配", ok, if ok { "" } else { "段破缺" });
    // J2 五元组（完整过 / 残缺拒 / 段外码拒）
    let mut wp = WatchPoint::default();
    let good = ErrTuple::new(XC_OOB, "CGPU-A", "site1", "成因", "建议");
    let bad = ErrTuple::new(XC_OOB, "CGPU-A", "site1", "", "");
    let outseg = ErrTuple::new(0x9999, "CGPU-A", "site1", "成因", "建议");
    let ok = escalate(&good, &mut wp).is_ok()
        && escalate(&bad, &mut wp) == Err(XC_TUPLE)
        && escalate(&outseg, &mut wp) == Err(XC_TUPLE)
        && !good.complete() == false
        && !bad.complete();
    s.add("CGA-41-02-五元组纪律", ok, if ok { "" } else { "残缺放行" });
    // J3 注入达观察点（三类注入全必达 + 零静默断言）
    let (injected, wp2) = injection_suite();
    let ok = injected.contains(&XC_OOB)
        && injected.contains(&XC_DEADLOCK)
        && injected.contains(&XC_SILENT)
        && injected.contains(&XC_TUPLE)
        && wp2.all_reached(&injected)
        && wp2.hits.len() >= 4;
    s.add("CGA-41-03-注入达观察点", ok, if ok { "" } else { "静默破缺" });
    // J3-b 零静默语义（未达码名单可机读）
    let missing: Vec<u16> = injected.iter().copied().filter(|c| !wp2.reached(*c)).collect();
    let ok = missing.is_empty();
    s.add("CGA-41-04-零静默断言", ok, if ok { "" } else { "名单非空" });
    // J4 联动复核——SIMD 防护（合法批过 / 末端恰满过 / 越界拒 / 溢出拒）
    let ok = simd_batch_write_guard(16, 0, 16).is_ok()
        && simd_batch_write_guard(16, 8, 8).is_ok()
        && simd_batch_write_guard(16, 8, 9) == Err(XC_OOB)
        && simd_batch_write_guard(16, usize::MAX, 1) == Err(XC_OOB);
    s.add("CGA-41-05-SIMD联动", ok, if ok { "" } else { "越界放行" });
    // J4-b 联动复核——死锁检测（无环过 / 环拒 / 越界任务号拒 / 大图无环过）
    let ok = pool_cycle_check(&[(1, 0), (2, 1)], 3).is_ok()
        && pool_cycle_check(&[(1, 0), (0, 1)], 2) == Err(XC_DEADLOCK)
        && pool_cycle_check(&[(5, 0)], 2) == Err(XC_DEADLOCK)
        && pool_cycle_check(&[], 16).is_ok();
    s.add("CGA-41-06-死锁联动", ok, if ok { "" } else { "闭环放行" });
    // J5 观察点账（码序列保序可审计）
    let mut wp3 = WatchPoint::default();
    wp3.hit(XC_OOB);
    wp3.hit(XC_DEADLOCK);
    wp3.hit(XC_OOB);
    let ok = wp3.hits == vec![XC_OOB, XC_DEADLOCK, XC_OOB] && wp3.reached(XC_OOB) && !wp3.reached(XC_TUPLE);
    s.add("CGA-41-07-观察点保序", ok, if ok { "" } else { "账序破缺" });
    // J6 码位互异段独占
    let ok = XC_SILENT != XC_TUPLE && XC_TUPLE != XC_OOB && XC_OOB != XC_DEADLOCK;
    s.add("CGA-41-08-码位互异", ok, if ok { "" } else { "码位冲突" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cga41_checks_all_green() {
        let set = run_cga41_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn cga41_no_silent_swallow() {
        // 上抛失败也必须留账（拒收码本身到达观察点）
        let mut wp = WatchPoint::default();
        let bad = ErrTuple::new(XC_SILENT, "", "", "", "");
        let _ = escalate(&bad, &mut wp);
        assert!(wp.reached(XC_TUPLE), "拒收必须留账");
    }
}
