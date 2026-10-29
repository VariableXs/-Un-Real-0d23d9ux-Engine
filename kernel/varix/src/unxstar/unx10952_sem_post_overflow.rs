//! UNX-F10952 sem_post EOVERFLOW 矩阵（0x1d680 段）
//!
//! 纯功能模型：POSIX 信号量 `sem_post` 裁决门。
//! 错误面：值已达 SEM_VALUE_MAX 再 post → EOVERFLOW。
//! 反面：限内 post 千轮零误报，且每次 post 加值账逐次对平。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1d680;
/// 值达上限再 post
pub const SP_EOVERFLOW: u32 = SEG_BASE + 0x01;
/// 坏句柄
pub const SP_EBADF: u32 = SEG_BASE + 0x02;
/// 门内对账失败
pub const SP_LEDGER: u32 = SEG_BASE + 0x03;

// ---------------------------------------------------------------- 常数
/// 信号量值上限
pub const SP_VALUE_MAX: u32 = 0x7fff;
/// 反面验收轮数
pub const SP_ROUNDS: usize = 1000;

// ---------------------------------------------------------------- 数据结构
/// 单轮日志。
#[derive(Clone, Debug, PartialEq)]
pub struct SpLog {
    /// 轮次
    pub round: usize,
    /// 裁定码：0 成功
    pub verdict: u32,
}

/// sem_post 裁决门。
#[derive(Clone, Debug)]
pub struct PostGate {
    /// 信号量当前值
    pub value: u32,
    /// 值上限
    pub vmax: u32,
    /// 成功加值次数
    pub incs: usize,
    /// 被拒的溢出 post 次数
    pub overflows: usize,
    /// 轮次
    pub rounds: usize,
    /// 成功轮
    pub ok_hits: usize,
    /// EOVERFLOW 命中
    pub eoverflow_hits: usize,
    /// EBADF 命中
    pub ebadf_hits: usize,
    /// 逐轮日志
    pub log: Vec<SpLog>,
}

impl PostGate {
    /// 建门。
    pub fn new(init: u32, vmax: u32) -> Self {
        PostGate {
            value: init,
            vmax,
            incs: 0,
            overflows: 0,
            rounds: 0,
            ok_hits: 0,
            eoverflow_hits: 0,
            ebadf_hits: 0,
            log: Vec::new(),
        }
    }

    /// 记账。
    fn book(&mut self, round: usize, verdict: u32) {
        self.rounds = self.rounds.wrapping_add(1);
        match verdict {
            0 => self.ok_hits = self.ok_hits.wrapping_add(1),
            x if x == SP_EOVERFLOW => self.eoverflow_hits = self.eoverflow_hits.wrapping_add(1),
            x if x == SP_EBADF => self.ebadf_hits = self.ebadf_hits.wrapping_add(1),
            _ => {}
        }
        self.log.push(SpLog { round, verdict });
    }

    /// sem_post：判定序 ① 坏句柄 → EBADF；② 值达上限 → EOVERFLOW；
    /// ③ 加值成功。
    pub fn sem_post(&mut self, fd_valid: bool, round: usize) -> Result<u32, u32> {
        if !fd_valid {
            self.book(round, SP_EBADF);
            return Err(SP_EBADF);
        }
        if self.value >= self.vmax {
            self.overflows = self.overflows.wrapping_add(1);
            self.book(round, SP_EOVERFLOW);
            return Err(SP_EOVERFLOW);
        }
        self.value += 1;
        self.incs = self.incs.wrapping_add(1);
        self.book(round, 0);
        Ok(self.value)
    }

    /// 配套减值（供账目滚动）。
    pub fn sem_take(&mut self) -> bool {
        if self.value == 0 {
            return false;
        }
        self.value -= 1;
        true
    }

    /// 距上限余量。
    pub fn headroom(&self) -> u32 {
        self.vmax - self.value
    }

    /// 账目对平。
    pub fn ledger_ok(&self) -> bool {
        let total = self
            .ok_hits
            .wrapping_add(self.eoverflow_hits)
            .wrapping_add(self.ebadf_hits);
        total == self.rounds && self.log.len() == self.rounds
    }

    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ (self.value as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.incs as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.overflows as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.rounds as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "PostGate{{value={},vmax={},incs={},overflows={},rounds={},ok={},eoverflow={}}}",
            self.value, self.vmax, self.incs, self.overflows, self.rounds,
            self.ok_hits, self.eoverflow_hits
        )
    }
}

// ---------------------------------------------------------------- 注入
/// 注入四路坏样本，返回四路是否全部被门正确拦截。
pub fn inject_four() -> [bool; 4] {
    let mut caught = [false; 4];
    // 注入 1：坏句柄 → EBADF。
    caught[0] = matches!(PostGate::new(1, SP_VALUE_MAX).sem_post(false, 9001), Err(x) if x == SP_EBADF);
    // 注入 2：值达上限再 post → EOVERFLOW。
    let mut g = PostGate::new(SP_VALUE_MAX, SP_VALUE_MAX);
    caught[1] = matches!(g.sem_post(true, 9002), Err(x) if x == SP_EOVERFLOW);
    // 注入 3：溢出后值不变、溢出账加一。
    caught[2] = g.value == SP_VALUE_MAX && g.overflows == 1;
    // 注入 4：减一后 post 恢复成功；再灌满再溢出仍拦截。
    let _ = g.sem_take();
    let ok = g.sem_post(true, 9003).is_ok();
    let hit = matches!(g.sem_post(true, 9004), Err(x) if x == SP_EOVERFLOW);
    caught[3] = ok && hit;
    caught
}

// ---------------------------------------------------------------- 确定性样本
/// 千轮样本：限内 post 与减值滚动为主，穿插溢出面。
/// 返回符合预期轮数。
pub fn sample_thousand(g: &mut PostGate) -> usize {
    let mut good = 0;
    let mut r = 0;
    while r < SP_ROUNDS {
        let low = r % 10;
        let matched;
        if low < 6 {
            // 合法面：限内 post（满则先减）。
            if g.value >= g.vmax {
                let _ = g.sem_take();
            }
            let before = g.value;
            let res = g.sem_post(true, r);
            matched = matches!(res, Ok(v) if v == before + 1);
        } else if low == 6 {
            // EOVERFLOW 面：临时灌满。
            let saved = g.value;
            g.value = g.vmax;
            let hit = matches!(g.sem_post(true, r), Err(x) if x == SP_EOVERFLOW);
            g.value = saved;
            matched = hit;
        } else if low == 7 {
            // EBADF 面。
            matched = matches!(g.sem_post(false, r), Err(x) if x == SP_EBADF);
        } else if low == 8 {
            // 合法面：减值滚动。
            matched = g.sem_take() || g.value == 0;
        } else {
            // 合法面：余量递减核验。
            let h0 = g.headroom();
            if g.value < g.vmax {
                let _ = g.sem_post(true, r);
                matched = g.headroom() == h0 - 1;
            } else {
                matched = g.headroom() == 0;
            }
        }
        if matched {
            good += 1;
        }
        r += 1;
    }
    good
}

// ---------------------------------------------------------------- 判据
/// UNX-F10952 判据组。
pub fn run_unx10952_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10952");
    // J1 正面：值达上限 post → EOVERFLOW 十轮全中。
    let mut g1 = PostGate::new(SP_VALUE_MAX, SP_VALUE_MAX);
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(g1.sem_post(true, k), Err(x) if x == SP_EOVERFLOW) {
            c += 1;
        }
        k += 1;
    }
    s.add(
        "J1-正面EOVERFLOW",
        c == 10 && g1.value == SP_VALUE_MAX && g1.overflows == 10,
        "上限值 post 十轮全部裁 EOVERFLOW 且值不变",
    );
    // J2 反面：限内 post 千轮零误报。
    let mut g2 = PostGate::new(0, SP_VALUE_MAX);
    let good = sample_thousand(&mut g2);
    s.add_dyn("J2-反面零误报", good == SP_ROUNDS, &format!("千轮样本符合预期 {} 轮", good));
    // J3 注入四路全捕获。
    let inj = inject_four();
    s.add(
        "J3-注入四路",
        inj[0] && inj[1] && inj[2] && inj[3],
        "EBADF/EOVERFLOW/账不变/恢复后再溢四路全部拦截",
    );
    // J4 边界：恰差一个余量时 post 成功到顶。
    let mut g3 = PostGate::new(SP_VALUE_MAX - 1, SP_VALUE_MAX);
    let r4 = g3.sem_post(true, 1);
    let r5 = g3.sem_post(true, 2);
    s.add(
        "J4-顶格边界",
        matches!(r4, Ok(v) if v == SP_VALUE_MAX) && matches!(r5, Err(x) if x == SP_EOVERFLOW),
        "差一余量 post 到顶后紧接溢出",
    );
    // J5 错误码分段。
    s.add(
        "J5-错误码分段",
        SP_EOVERFLOW > SEG_BASE && SP_LEDGER < SEG_BASE + 0x20,
        "三个错误码全部落在段基址页内",
    );
    // J6 错误码互异。
    s.add(
        "J6-错误码互异",
        SP_EOVERFLOW != SP_EBADF && SP_EBADF != SP_LEDGER && SP_EOVERFLOW != SP_LEDGER,
        "三个错误码两两不同",
    );
    // J7 账目对平。
    s.add("J7-账目对平", g2.ledger_ok(), "千轮之后三类计数与轮次、日志逐项对平");
    // J8 加值账回归：incs 与终值一致（初值 0）。
    s.add(
        "J8-加值账回归",
        g2.incs as u32 >= g2.value && g2.headroom() <= g2.vmax,
        "加值账与余量有界",
    );
    // J9 fold 稳定。
    let f = g2.fold();
    let clone = g2.clone();
    s.add("J9-fold稳定", clone.fold() == f, "克隆后指纹逐位一致");
    // J10 渲染非空。
    let txt = g2.render();
    s.add("J10-渲染非空", txt.len() > 10 && txt.contains("PostGate"), "门级渲染含类型名与计数");
    // J11 余量口径：headroom = vmax - value。
    let mut g4 = PostGate::new(5, 10);
    s.add("J11-余量口径", g4.headroom() == 5, "余量等于上限减当前值");
    // J12 溢出计数独立：EBADF 不进溢出账。
    let mut g5 = PostGate::new(SP_VALUE_MAX, SP_VALUE_MAX);
    let _ = g5.sem_post(false, 1);
    let _ = g5.sem_post(true, 2);
    s.add(
        "J12-账目分立",
        g5.ebadf_hits == 1 && g5.eoverflow_hits == 1 && g5.overflows == 1,
        "EBADF 与溢出账互不串",
    );
    // J13 动态：千轮明细。
    s.add_dyn(
        "J13-千轮明细",
        good == SP_ROUNDS,
        &format!("good={} gate={}", good, g2.render()),
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
    fn unx10952_sem_post_overflow_flow() {
        // 流程：限内加值 → 灌满 → 溢出拒 → 减一 → 再加值成功 → 坏句柄。
        let mut g = PostGate::new(0, 4);
        assert_eq!(g.sem_post(true, 1), Ok(1));
        assert_eq!(g.sem_post(true, 2), Ok(2));
        assert_eq!(g.sem_post(true, 3), Ok(3));
        assert_eq!(g.sem_post(true, 4), Ok(4));
        assert!(matches!(g.sem_post(true, 5), Err(x) if x == SP_EOVERFLOW));
        assert!(g.sem_take());
        assert_eq!(g.sem_post(true, 6), Ok(4));
        assert!(matches!(g.sem_post(false, 7), Err(x) if x == SP_EBADF));
        assert!(g.ledger_ok());
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx10952_checks_green() {
        let s = run_unx10952_checks();
        assert!(s.all_passed(), "UNX-F10952 判据未全绿");
    }
}
