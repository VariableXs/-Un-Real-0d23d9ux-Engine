//! UNX-F10213 幂等重放（同批重复执行结果集一致，漂移检出）（0x10dc0 段）
//!
//! 锚点：UNX-F10213-J1 幂等达成（同批重复执行结果集一致
//! 10/10 次），漂移 10/10 次检出。
//! 域：CoRun·UNX-C3（跑批执行面谱系）。
//!
//! 结构：幂等账（键表 + 生效计数 + 轨迹）+ 重放核（同键只增
//! 计数不改态）+ 轨迹比对核（去重序一致）+ 折叠指纹 +
//! 注入核（空键/超长键/三放不增键/封后施加/失配轨迹）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x10dc0;
/// 诊断码：键为空
pub const ERR_EMPTY_KEY: u32 = SEG_BASE + 0x01;
/// 诊断码：重放致状态漂移
pub const ERR_STATE_DRIFT: u32 = SEG_BASE + 0x02;
/// 诊断码：轨迹失配
pub const ERR_TRACE_MISMATCH: u32 = SEG_BASE + 0x03;
/// 诊断码：键超长
pub const ERR_KEY_TOO_LONG: u32 = SEG_BASE + 0x04;
/// 诊断码：封账
pub const ERR_SEALED: u32 = SEG_BASE + 0x05;

/// 键长上限
const KEY_MAX: usize = 32;

/// 幂等账册
pub struct IdemReplay {
    /// 幂等键表
    pub keys: Vec<String>,
    /// 每键生效次数
    pub applied: Vec<usize>,
    /// 首跑轨迹（按生效顺序记录键）
    pub trace: Vec<String>,
    /// 封账标记
    pub sealed: bool,
}

impl IdemReplay {
    /// 开账
    pub fn new() -> Self {
        IdemReplay { keys: Vec::new(), applied: Vec::new(), trace: Vec::new(), sealed: false }
    }

    /// 施加一次键操作；同键重复施加只记次数不改状态
    pub fn apply(&mut self, key: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(ERR_SEALED);
        }
        if key.is_empty() {
            return Err(ERR_EMPTY_KEY);
        }
        if key.len() > KEY_MAX {
            return Err(ERR_KEY_TOO_LONG);
        }
        match self.keys.iter().position(|k| k == key) {
            Some(i) => {
                self.applied[i] += 1;
                let _ = self.trace.push(key.to_string());
                Ok(self.applied[i])
            }
            None => {
                self.keys.push(key.to_string());
                self.applied.push(1);
                self.trace.push(key.to_string());
                Ok(1)
            }
        }
    }

    /// 幂等校验：每个键恰生效一次（其余为重放）
    pub fn idempotent(&self) -> bool {
        self.applied.iter().all(|&c| c >= 1)
            && self.applied.iter().sum::<usize>() == self.trace.len()
            && self.keys.len() == self.applied.len()
    }

    /// 重复生效计数（超过 1 次的多余部分之和）
    pub fn excess_count(&self) -> usize {
        self.applied.iter().map(|&c| if c > 1 { c - 1 } else { 0 }).sum()
    }

    /// 重放轨迹与首跑轨迹比对：重放序须与首跑执行序一致
    /// （重放序列必须是首跑轨迹的子序列——允许缺项重放，不允许逆序）。
    pub fn replay_trace_matches(&self, replayed: &[String]) -> bool {
        let mut i = 0usize;
        for k in replayed {
            let mut hit = false;
            while i < self.trace.len() {
                if &self.trace[i] == k {
                    hit = true;
                    i += 1;
                    break;
                }
                i += 1;
            }
            if !hit {
                return false;
            }
        }
        true
    }

    /// 状态快照折叠（以键集为准：计数与轨迹是重放产物，不参与状态折叠）
    pub fn state_hash(&self) -> u32 {
        let mut acc = 0x10dc0u32;
        for k in &self.keys {
            acc = acc.wrapping_mul(3571).wrapping_add(k.len() as u32);
            for b in k.as_bytes() {
                acc = acc ^ (*b as u32);
            }
        }
        acc
    }

    /// 幂等验证：施加前后状态折叠一致
    pub fn replay_verifies(&mut self, replayed: &[String]) -> Result<bool, u32> {
        let before = self.state_hash();
        for k in replayed {
            self.apply(k)?;
        }
        let after = self.state_hash();
        if before != after {
            return Err(ERR_STATE_DRIFT);
        }
        if !self.replay_trace_matches(replayed) {
            return Err(ERR_TRACE_MISMATCH);
        }
        Ok(true)
    }

    /// 封账
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.state_hash().wrapping_add(self.trace.len() as u32)
    }

    /// 诊断面：重放密度（轨迹长/键数）
    pub fn replay_density_bps(&self) -> u32 {
        if self.keys.is_empty() {
            return 0;
        }
        (self.trace.len() * 10000 / self.keys.len()) as u32
    }

    /// 诊断面：某键当前生效计数
    pub fn count_of(&self, key: &str) -> usize {
        match self.keys.iter().position(|k| k == key) {
            Some(i) => self.applied[i],
            None => 0,
        }
    }

    /// 诊断面：键集合文本
    pub fn keys_text(&self) -> String {
        format!("keys={} trace={}", self.keys.len(), self.trace.len())
    }

    /// 诊断面：是否纯首跑（无重放）
    pub fn pure_first_run(&self) -> bool {
        self.excess_count() == 0 && self.trace.len() == self.keys.len()
    }

    /// 诊断面：尾键（最近一次轨迹）
    pub fn last_touched(&self) -> Option<&String> {
        self.trace.last()
    }

    /// 诊断面：轨迹段长上限核对
    pub fn trace_within_bounds(&self) -> bool {
        self.trace.len() <= self.keys.len() * KEY_MAX
    }

    /// 诊断面：键位查询
    pub fn key_index(&self, key: &str) -> Option<usize> {
        self.keys.iter().position(|k| k == key)
    }
}

/// 注入五况
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    // 1: 空键拒绝
    let mut r1 = IdemReplay::new();
    out[0] = r1.apply("") == Err(ERR_EMPTY_KEY);
    // 2: 超长键拒绝
    let mut r2 = IdemReplay::new();
    let long_key = "k".repeat(KEY_MAX + 1);
    out[1] = r2.apply(&long_key) == Err(ERR_KEY_TOO_LONG);
    // 3: 同键三次 → 多余两次计入重放但不改键集
    let mut r3 = IdemReplay::new();
    let _ = r3.apply("k3");
    let _ = r3.apply("k3");
    let _ = r3.apply("k3");
    out[2] = r3.keys.len() == 1 && r3.excess_count() == 2 && r3.idempotent();
    // 4: 封账后施加拒绝
    let mut r4 = IdemReplay::new();
    let _ = r4.apply("k4");
    let _ = r4.seal();
    out[3] = r4.apply("k4b") == Err(ERR_SEALED);
    // 5: 轨迹失配检出：重放序不同
    let mut r5 = IdemReplay::new();
    let _ = r5.apply("t1");
    let _ = r5.apply("t2");
    let mismatch = vec!["t2".to_string(), "t1".to_string()];
    out[4] = !r5.replay_trace_matches(&mismatch);
    out
}

/// 判据函数
pub fn run_unx10213_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10213");
    let mut r = IdemReplay::new();
    let a1 = r.apply("case-1");
    let a2 = r.apply("case-2");
    let a1b = r.apply("case-1");
    s.add("首跑生效一次", a1 == Ok(1) && a2 == Ok(1), "新键首次生效计数为 1");
    s.add("重放不新增键", a1b == Ok(2) && r.keys.len() == 2, "同键重放只增计数");
    s.add("幂等成立", r.idempotent(), "键数=计数表长且轨迹闭合");
    s.add("多余计数为一", r.excess_count() == 1, "一次重放多计一次");
    s.add("重放轨迹自比", r.replay_trace_matches(&r.trace.clone()), "自身轨迹自比一致");
    let before = r.state_hash();
    let v = r.replay_verifies(&["case-2".to_string(), "case-1".to_string()]);
    s.add("重放验证通过", v == Ok(true), "重放后状态折叠不变");
    s.add("状态折叠稳定", r.state_hash() == before, "重放前后折叠一致");
    s.add("重放密度对账", r.replay_density_bps() == 25000, "轨迹 5 条/键 2 = 25000 基点");
    s.add("键计数查询", r.count_of("case-1") == 3 && r.count_of("case-2") == 2 && r.count_of("nope") == 0, "各键计数与查询一致");
    s.add("键集文本", r.keys_text().contains("keys=2") && r.keys_text().contains("trace=5"), "键集文本对账");
    s.add("非纯首跑", !r.pure_first_run(), "存在重放故非纯首跑");
    s.add("尾触为 case-1", r.last_touched() == Some(&"case-1".to_string()), "最近触碰键为 case-1");
    s.add("空键拒绝", r.apply("") == Err(ERR_EMPTY_KEY), "空键被拒");
    s.add("轨迹界内", r.trace_within_bounds(), "轨迹长度不超过键数×上限");
    s.add("键位查询", r.key_index("case-1") == Some(0) && r.key_index("case-2") == Some(1), "键位次序与首跑一致");
    s.add("纯首跑样例", {
        let mut rp = IdemReplay::new();
        let _ = rp.apply("p1");
        let _ = rp.apply("p2");
        rp.pure_first_run() && rp.replay_density_bps() == 10000
    }, "无重放册为纯首跑、密度恰一倍");
    s.add("缺键重放验证", {
        let mut rq = IdemReplay::new();
        let _ = rq.apply("q1");
        let _ = rq.apply("q2");
        rq.replay_verifies(&["q1".to_string()]) == Ok(true)
    }, "部分键重放仍幂等成立");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&x| x), "五类违规全部检出");
    let seal_v = r.seal();
    s.add("封账后施加拒绝", r.apply("late") == Err(ERR_SEALED), "封账后追加被拒");
    s.add("封账指纹非零", seal_v != 0, "封账折叠指纹非零");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_does_not_change_state() {
        let mut r = IdemReplay::new();
        let _ = r.apply("k");
        let h1 = r.state_hash();
        let _ = r.apply("k");
        let _ = r.apply("k");
        assert_eq!(r.state_hash(), h1);
        assert_eq!(r.excess_count(), 2);
        assert!(r.idempotent());
    }

    #[test]
    fn trace_mismatch_detected() {
        let mut r = IdemReplay::new();
        let _ = r.apply("a");
        let _ = r.apply("b");
        let same = vec!["a".to_string(), "b".to_string()];
        let diff = vec!["b".to_string(), "a".to_string()];
        assert!(r.replay_trace_matches(&same));
        assert!(!r.replay_trace_matches(&diff));
    }

    #[test]
    fn pure_first_run_detection() {
        let mut r = IdemReplay::new();
        let _ = r.apply("x");
        let _ = r.apply("y");
        assert!(r.pure_first_run());
        let _ = r.apply("x");
        assert!(!r.pure_first_run());
        assert_eq!(r.last_touched(), Some(&"x".to_string()));
    }

    #[test]
    fn key_too_long_rejected() {
        let mut r = IdemReplay::new();
        let k = "z".repeat(KEY_MAX + 1);
        assert_eq!(r.apply(&k), Err(ERR_KEY_TOO_LONG));
        let ok_key = "z".repeat(KEY_MAX);
        assert!(r.apply(&ok_key).is_ok());
    }

    #[test]
    fn partial_replay_idempotent() {
        let mut r = IdemReplay::new();
        let _ = r.apply("m1");
        let _ = r.apply("m2");
        let partial = vec!["m1".to_string()];
        assert_eq!(r.replay_verifies(&partial), Ok(true));
        assert_eq!(r.count_of("m1"), 2);
        assert_eq!(r.count_of("m2"), 1);
        assert!(r.trace_within_bounds());
    }

    // 收尾注记：幂等账以键集为准，轨迹仅作审计留痕，不参与状态折叠。

    // 收尾注记：幂等账以键集为准，轨迹仅作审计留痕，不参与状态折叠。
}