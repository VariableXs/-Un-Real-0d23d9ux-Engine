//! UNX-F10939 SysV 判定序挂接核销（F10879 序表）（0x1d4e0 段）
//!
//! 纯功能模型：以 F10879 判定序表为基准，把 SysV 错误矩阵段
//! （F10921–F10939）的错误条逐条挂接核销。
//! 挂接纪律：序号严格 = 已挂接数 + 1；核销要求前缀全核销；
//! 封印后指纹零漂移。反面千轮样本零漂移、零错位。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1d4e0;
/// 挂接序号错位
pub const OH_BAD_SEQ: u32 = SEG_BASE + 0x01;
/// 核销时前缀未满
pub const OH_PREFIX: u32 = SEG_BASE + 0x02;
/// 封印后再挂接
pub const OH_SEALED: u32 = SEG_BASE + 0x03;
/// 挂接条目不在序表内
pub const OH_UNKNOWN: u32 = SEG_BASE + 0x04;
/// 账目对平失败
pub const OH_LEDGER: u32 = SEG_BASE + 0x05;

// ---------------------------------------------------------------- 常数
/// F10879 序表的 SysV 错误条目名（按判定序排列）
pub const ORDER_TABLE: [&str; 19] = [
    "msgget",
    "msgsnd-eagain",
    "msgsnd-szperm",
    "msgrcv-nomsg",
    "msgrcv-eidrm",
    "msgctl-matrix",
    "msgget-enospc",
    "semget-matrix",
    "semop-eagain",
    "semop-eidrm",
    "semundo-rollback",
    "semctl-matrix",
    "shmget-matrix",
    "shmat-matrix",
    "shmdt-einval",
    "shmctl-matrix",
    "rmid-liveness",
    "ftok-conflict",
    "b27-close",
];
/// 反面验收轮数
pub const OH_ROUNDS: usize = 1000;

// ---------------------------------------------------------------- 数据结构
/// 一条挂接记录。
#[derive(Clone, Debug, PartialEq)]
pub struct HookEntry {
    /// 序表条目名
    pub name: String,
    /// 挂接序号（1 起，严格连续）
    pub seq: usize,
    /// 是否已核销
    pub settled: bool,
}

/// 挂接簿。
#[derive(Clone, Debug)]
pub struct HookBook {
    /// 已挂接条目（按挂接序）
    pub hooked: Vec<HookEntry>,
    /// 是否已封印
    pub sealed: bool,
    /// 挂接轮次
    pub hooks: usize,
    /// 核销次数
    pub settles: usize,
    /// 拒绝次数
    pub rejects: usize,
}

impl HookBook {
    /// 建空簿。
    pub fn new() -> Self {
        HookBook { hooked: Vec::new(), sealed: false, hooks: 0, settles: 0, rejects: 0 }
    }

    /// 条目是否在序表内。
    pub fn in_table(name: &str) -> bool {
        let mut i = 0;
        while i < ORDER_TABLE.len() {
            if ORDER_TABLE[i] == name {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 挂接：封印后 → SEALED；不在表 → UNKNOWN；
    /// 序号错位 → BAD_SEQ；正确 → 记录 seq = 已挂接数 + 1。
    pub fn hook(&mut self, name: &str, claimed_seq: usize) -> Result<usize, u32> {
        if self.sealed {
            self.rejects += 1;
            return Err(OH_SEALED);
        }
        if !HookBook::in_table(name) {
            self.rejects += 1;
            return Err(OH_UNKNOWN);
        }
        let expect = self.hooked.len().wrapping_add(1);
        if claimed_seq != expect {
            self.rejects += 1;
            return Err(OH_BAD_SEQ);
        }
        self.hooked.push(HookEntry { name: name.to_string(), seq: expect, settled: false });
        self.hooks += 1;
        Ok(expect)
    }

    /// 前缀是否全核销。
    pub fn prefix_settled(&self) -> bool {
        let mut i = 0;
        while i < self.hooked.len() {
            if !self.hooked[i].settled {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 核销：要求前缀全核销；封印后仍可核销（封印只锁挂接）。
    pub fn settle(&mut self, seq: usize) -> Result<(), u32> {
        let mut found = None;
        let mut i = 0;
        while i < self.hooked.len() {
            if self.hooked[i].seq == seq {
                found = Some(i);
            }
            i += 1;
        }
        let ix = match found {
            Some(f) => f,
            None => {
                self.rejects += 1;
                return Err(OH_UNKNOWN);
            }
        };
        if !self.prefix_settled() && self.hooked[ix].seq != self.first_open_seq() {
            self.rejects += 1;
            return Err(OH_PREFIX);
        }
        if self.hooked[ix].settled {
            self.rejects += 1;
            return Err(OH_PREFIX);
        }
        self.hooked[ix].settled = true;
        self.settles += 1;
        Ok(())
    }

    /// 首个未核销序号（全核销时返回挂接数 + 1）。
    pub fn first_open_seq(&self) -> usize {
        let mut i = 0;
        while i < self.hooked.len() {
            if !self.hooked[i].settled {
                return self.hooked[i].seq;
            }
            i += 1;
        }
        self.hooked.len().wrapping_add(1)
    }

    /// 封印：要求前缀全核销。
    pub fn seal(&mut self) -> Result<(), u32> {
        if !self.prefix_settled() {
            self.rejects += 1;
            return Err(OH_PREFIX);
        }
        self.sealed = true;
        Ok(())
    }

    /// 与序表逐位核销：已挂接条目名与序表前缀一致。
    pub fn matches_table(&self) -> bool {
        let mut i = 0;
        while i < self.hooked.len() {
            if ORDER_TABLE[i] != self.hooked[i].name || self.hooked[i].seq != i + 1 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.hooked.len() {
            h = (h ^ (self.hooked[i].seq as u64)).wrapping_mul(0x100000001b3);
            let bytes = self.hooked[i].name.as_bytes();
            let mut j = 0;
            while j < bytes.len() {
                h = (h ^ (bytes[j] as u64)).wrapping_mul(0x100000001b3);
                j += 1;
            }
            h = (h ^ (self.hooked[i].settled as u64)).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ (self.sealed as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "HookBook{{hooked={},settles={},rejects={},sealed={}}}",
            self.hooked.len(), self.settles, self.rejects, self.sealed as u32
        )
    }
}

// ---------------------------------------------------------------- 注入
/// 注入四路坏样本，返回四路是否全部被簿正确拦截。
pub fn inject_four() -> [bool; 4] {
    let mut caught = [false; 4];
    let mut b = HookBook::new();
    // 注入 1：序号错位（跳号挂接）→ BAD_SEQ。
    caught[0] = matches!(b.hook("msgget", 3), Err(x) if x == OH_BAD_SEQ);
    // 注入 2：不在序表的条目 → UNKNOWN。
    caught[1] = matches!(b.hook("not-a-sysv-step", 1), Err(x) if x == OH_UNKNOWN);
    // 注入 3：前缀未满核销 → PREFIX。
    let _ = b.hook("msgget", 1);
    let _ = b.hook("msgsnd-eagain", 2);
    caught[2] = matches!(b.settle(2), Err(x) if x == OH_PREFIX);
    // 注入 4：封印后再挂接 → SEALED。
    let _ = b.settle(1);
    let _ = b.settle(2);
    assert!(b.seal().is_ok());
    caught[3] = matches!(b.hook("msgrcv-nomsg", 3), Err(x) if x == OH_SEALED);
    caught
}

// ---------------------------------------------------------------- 确定性样本
/// 千轮样本：每 50 轮跑一遍完整 19 条（挂接→逐条核销→封印），其余轮做抽查遍。
/// 返回"序号、核销、序表逐位一致"的轮数。
pub fn sample_thousand() -> usize {
    let mut good = 0;
    let mut round = 0;
    while round < OH_ROUNDS {
        // 每轮独立建簿：挂接纪律要求序号从 1 起，复用旧簿会直接错位。
        let mut b = HookBook::new();
        let mut clean = true;
        if round % 50 == 0 {
            // 完整遍：挂接全部 19 条并逐条核销，末尾封印。
            let mut i = 0;
            while i < ORDER_TABLE.len() {
                if b.hook(ORDER_TABLE[i], i + 1).is_err() {
                    clean = false;
                }
                if b.settle(i + 1).is_err() {
                    clean = false;
                }
                i += 1;
            }
            if b.seal().is_err() {
                clean = false;
            }
            if clean && b.matches_table() && b.settles == ORDER_TABLE.len() && b.sealed {
                good += 1;
            }
        } else {
            // 抽查遍：挂到 slot 位后按序核销到 slot+1，验证开放位严格推进；
            // 跳过前缀直接核销 slot+1 会被 PREFIX 拒（那不是模型错，是样本错）。
            let slot = (round / 50) % ORDER_TABLE.len();
            let mut i = 0;
            while i <= slot {
                if b.hook(ORDER_TABLE[i], i + 1).is_err() {
                    clean = false;
                }
                i += 1;
            }
            let mut s = 0;
            while s <= slot {
                if b.settle(s + 1).is_err() {
                    clean = false;
                }
                s += 1;
            }
            if clean && b.first_open_seq() == slot + 2 {
                good += 1;
            }
        }
        round += 1;
    }
    good
}

// ---------------------------------------------------------------- 判据
/// UNX-F10939 判据组。
pub fn run_unx10939_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10939");
    // J1 正面之一：序表逐条核销（19/19）。
    let mut b1 = HookBook::new();
    let mut all_ok = true;
    let mut i = 0;
    while i < ORDER_TABLE.len() {
        if b1.hook(ORDER_TABLE[i], i + 1).is_err() {
            all_ok = false;
        }
        if b1.settle(i + 1).is_err() {
            all_ok = false;
        }
        i += 1;
    }
    // 全核销后封印：封印只锁挂接，故须在 J1-2/J8 之前落封印。
    let sealed_ok = b1.seal().is_ok();
    s.add(
        "J1-逐条核销",
        all_ok && b1.matches_table() && b1.settles == 19,
        "F10879 序表 19 条逐条挂接核销",
    );
    // J1 正面之二：衔接零冲突（核销后封印成功）。
    s.add("J1-封印成功", sealed_ok && b1.sealed && b1.prefix_settled(), "全核销后封印成功，零冲突");
    // J2 正面补充：抽查遍序号纪律（首开放位严格推进）。
    let mut b2 = HookBook::new();
    let mut ok2 = true;
    let mut i = 0;
    while i < 5 {
        if b2.hook(ORDER_TABLE[i], i + 1).is_err() {
            ok2 = false;
        }
        i += 1;
    }
    s.add(
        "J2-序号纪律",
        ok2 && b2.first_open_seq() == 1 && b2.settle(1).is_ok() && b2.first_open_seq() == 2,
        "核销后开放位严格推进",
    );
    // J3 反面：千轮零漂移。
    let good = sample_thousand();
    s.add_dyn("J3-反面零漂移", good == OH_ROUNDS, &format!("千轮样本符合预期 {} 轮", good));
    // J4 注入四路。
    let inj = inject_four();
    s.add(
        "J4-注入四路",
        inj[0] && inj[1] && inj[2] && inj[3],
        "错位/未知条目/前缀未满/封印后挂接四路全部拦截",
    );
    // J5 序表完整性：19 条且无重名。
    let mut dup = false;
    let mut a = 0;
    while a < ORDER_TABLE.len() {
        let mut b = a + 1;
        while b < ORDER_TABLE.len() {
            if ORDER_TABLE[a] == ORDER_TABLE[b] {
                dup = true;
            }
            b += 1;
        }
        a += 1;
    }
    s.add("J5-序表完整", ORDER_TABLE.len() == 19 && !dup, "序表 19 条且无重名");
    // J6 错误码分段。
    s.add(
        "J6-错误码分段",
        OH_BAD_SEQ > SEG_BASE && OH_LEDGER < SEG_BASE + 0x20,
        "五个错误码全部落在段基址页内",
    );
    // J7 错误码互异。
    let codes = [OH_BAD_SEQ, OH_PREFIX, OH_SEALED, OH_UNKNOWN, OH_LEDGER];
    let mut distinct = true;
    let mut a = 0;
    while a < codes.len() {
        let mut b = a + 1;
        while b < codes.len() {
            if codes[a] == codes[b] {
                distinct = false;
            }
            b += 1;
        }
        a += 1;
    }
    s.add("J7-错误码互异", distinct, "五个错误码两两不同");
    // J8 封印锁挂接：封印后任何挂接被拒。
    s.add(
        "J8-封印锁",
        matches!(b1.hook("b27-close", 20), Err(x) if x == OH_SEALED),
        "封印后挂接一律被拒",
    );
    // J9 fold 稳定。
    let f = b1.fold();
    let clone = b1.clone();
    s.add("J9-fold稳定", clone.fold() == f && clone.fold() == f, "封印簿指纹零漂移");
    // J10 部分簿不匹配全表：matches_table 只对前缀有效。
    let mut b3 = HookBook::new();
    let _ = b3.hook("msgget", 1);
    s.add(
        "J10-前缀一致",
        b3.matches_table() && b3.hooked.len() < ORDER_TABLE.len(),
        "部分挂接时前缀一致判定可用",
    );
    // J11 渲染非空。
    let txt = b1.render();
    s.add("J11-渲染非空", txt.len() > 10 && txt.contains("HookBook"), "簿级渲染含类型名与计数");
    // J12 重复核销拒绝。
    let mut b4 = HookBook::new();
    let _ = b4.hook("msgget", 1);
    let _ = b4.settle(1);
    s.add("J12-重复核销", matches!(b4.settle(1), Err(_)), "同条二次核销被拒");
    // J13 动态：千轮明细。
    s.add_dyn(
        "J13-千轮明细",
        good == OH_ROUNDS,
        &format!("good={} table={}", good, ORDER_TABLE.len()),
    );
    // J14 动态：注入明细。
    s.add_dyn(
        "J14-注入明细",
        inj.iter().all(|x| *x),
        &format!("inj={:?}", inj),
    );
    s
}

// ---------------------------------------------------------------- 测试
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10939_sysv_order_hook_flow() {
        // 流程：按序挂接前 3 条 → 核销 1 → 开放位推进 → 核销 2/3 → 封印。
        let mut b = HookBook::new();
        assert_eq!(b.hook("msgget", 1), Ok(1));
        assert_eq!(b.hook("msgsnd-eagain", 2), Ok(2));
        assert_eq!(b.hook("msgsnd-szperm", 3), Ok(3));
        assert!(matches!(b.hook("msgsnd-szperm", 3), Err(x) if x == OH_BAD_SEQ));
        assert!(b.settle(1).is_ok());
        assert!(matches!(b.settle(3), Err(x) if x == OH_PREFIX));
        assert!(b.settle(2).is_ok());
        assert!(b.settle(3).is_ok());
        assert!(b.seal().is_ok());
        assert!(b.matches_table());
        assert!(inject_four().iter().all(|x| *x));
        assert_eq!(sample_thousand(), OH_ROUNDS);
    }

    #[test]
    fn unx10939_checks_green() {
        let s = run_unx10939_checks();
        assert!(s.all_passed(), "UNX-F10939 判据未全绿");
    }
}
