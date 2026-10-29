//! UNX-F10984 读端 kill 写者 EPIPE/SIGPIPE 全链压力（0x1da80 段）
//!
//! 锚点：UNX-F10984-J1 kill 读端写者三态千次对账（10/10），组内隔离
//! 对平（10/10）。
//! 域：CoRun·UNX-B30（I 型联测域·管道断端面）。
//!
//! 结构：态账（三态：EPIPE 返错 / SIGPIPE 倒下 / 忽略信号后 EPIPE）+
//! 组账（三写者一组，逐组记写次/返错/倒下/存活）+ 断端器（每轮先杀
//! 读端再让三写者各写一次）+ 隔离闸（组账不相借贷、三态之和等于写次）+
//! fold 与冻结 + 注入核（坏态号/越界组号/冻结后写/断端前写四路必抓）。
//!
//! 三态律：kill 读端后写者只可能落三态之一 —— 返 EPIPE 且存活、
//! 收 SIGPIPE 倒下、忽略 SIGPIPE 后返 EPIPE 且存活。三态之和必须
//! 等于写次，不得串态、不得凭空增损。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：10984 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1da80;

/// 空账 —— 未立组即写
pub const KS_EMPTY: u32 = SEG_BASE + 0x01;
/// 组号越界 —— 三组 0..3
pub const KS_RANGE: u32 = SEG_BASE + 0x02;
/// 重号立组 —— 同一组只准一条
pub const KS_DUP: u32 = SEG_BASE + 0x03;
/// 坏态号 —— 态号 1..=3
pub const KS_BADKIND: u32 = SEG_BASE + 0x04;
/// 隔离失守 —— 组账相互借贷
pub const KS_LEAK: u32 = SEG_BASE + 0x05;
/// 账已冻结 —— freeze 后禁止再写
pub const KS_FROZEN: u32 = SEG_BASE + 0x06;

/// 态：写者返 EPIPE 并存活。
pub const KIND_EPIPE: u8 = 1;
/// 态：写者收 SIGPIPE 倒下。
pub const KIND_SIGPIPE: u8 = 2;
/// 态：忽略 SIGPIPE 后返 EPIPE 并存活。
pub const KIND_IGNORED: u8 = 3;
/// 态数。
pub const KINDS: usize = 3;
/// 写者组数（三写者一组）。
pub const GROUPS: usize = 3;
/// 每组写者数。
pub const WRITERS_PER_GROUP: u64 = 3;
/// 对账轮数（千次）。
pub const ROUNDS: u64 = 1000;
/// 断端账域标（fold 种子）。
pub const KS_TAG: u64 = 0x1da_84;

/// 态账：一种断端态的四段计数。
#[derive(Clone, Debug)]
pub struct KillState {
    /// 态号（0x15900 段起编）
    pub kid: u32,
    /// 态序（1..=3）
    pub kind: u8,
    /// 态名
    pub name: String,
    /// 写次
    pub writes: u64,
    /// 返错次数
    pub epipes: u64,
    /// 倒下次数
    pub sigpipes: u64,
    /// 存活次数
    pub survived: u64,
}

impl KillState {
    /// 态内对平：写次 == 返错 + 倒下。
    pub fn balanced(&self) -> bool {
        self.writes == self.epipes.wrapping_add(self.sigpipes)
    }

    /// 存活与倒下之和等于写次。
    pub fn total_ok(&self) -> bool {
        self.survived.wrapping_add(self.sigpipes) == self.writes
    }

    /// 该态是否应当存活（EPIPE / 忽略态存活，SIGPIPE 态倒下）。
    pub fn should_survive(&self) -> bool {
        self.kind == KIND_EPIPE || self.kind == KIND_IGNORED
    }

    /// 存活语义与态号一致。
    pub fn semantics(&self) -> bool {
        if self.should_survive() {
            self.survived == self.writes && self.sigpipes == 0
        } else {
            self.sigpipes == self.writes && self.survived == 0
        }
    }
}

/// 组账：一组写者的隔离留痕。
#[derive(Clone, Debug)]
pub struct WriterGroup {
    /// 组号（0x15a00 段起编）
    pub gid: u32,
    /// 组序（0..3）
    pub no: u8,
    /// 组写次
    pub writes: u64,
    /// 组返错
    pub epipes: u64,
    /// 组倒下
    pub sigpipes: u64,
}

impl WriterGroup {
    /// 组内对平。
    pub fn balanced(&self) -> bool {
        self.writes == self.epipes.wrapping_add(self.sigpipes)
    }

    /// 组是否全倒下（无存活）。
    pub fn all_down(&self) -> bool {
        self.epipes == 0 && self.sigpipes == self.writes && self.writes > 0
    }
}

/// 断端全链总账。
#[derive(Clone, Debug)]
pub struct KillBook {
    /// 三态账
    pub states: Vec<KillState>,
    /// 三组账
    pub groups: Vec<WriterGroup>,
    /// 已跑轮数
    pub rounds: u64,
    /// 累计写次
    pub writes: u64,
    /// 累计返错
    pub epipes: u64,
    /// 累计倒下
    pub sigpipes: u64,
    /// 断端标记（读端是否已杀）
    pub killed: bool,
    /// 冻结标记
    pub frozen: bool,
}

impl KillBook {
    /// 立账：三态三组全部登记（零计数，未断端）。
    pub fn new() -> KillBook {
        let mut b = KillBook {
            states: Vec::new(),
            groups: Vec::new(),
            rounds: 0,
            writes: 0,
            epipes: 0,
            sigpipes: 0,
            killed: false,
            frozen: false,
        };
        let names = ["EPIPE", "SIGPIPE", "IGNORED"];
        let mut k = 0usize;
        while k < KINDS {
            b.states.push(KillState {
                kid: 0x15900u32.wrapping_add(k as u32),
                kind: (k as u8) + 1,
                name: names[k].to_string(),
                writes: 0,
                epipes: 0,
                sigpipes: 0,
                survived: 0,
            });
            k += 1;
        }
        let mut j = 0usize;
        while j < GROUPS {
            b.groups.push(WriterGroup {
                gid: 0x15a00u32.wrapping_add(j as u32),
                no: j as u8,
                writes: 0,
                epipes: 0,
                sigpipes: 0,
            });
            j += 1;
        }
        b
    }

    /// 态定位。
    pub fn state_ix(&self, kind: u8) -> Result<usize, u32> {
        if self.states.is_empty() {
            return Err(KS_EMPTY);
        }
        if kind == 0 || kind as usize > KINDS {
            return Err(KS_BADKIND);
        }
        Ok((kind as usize).wrapping_sub(1))
    }

    /// 组定位。
    pub fn group_ix(&self, no: usize) -> Result<usize, u32> {
        if self.groups.is_empty() {
            return Err(KS_EMPTY);
        }
        if no >= self.groups.len() {
            return Err(KS_RANGE);
        }
        Ok(no)
    }

    /// 轮号 → 断端态（三态轮转）。
    pub fn kind_at(round: u64) -> u8 {
        ((round % 3) as u8).wrapping_add(1)
    }

    /// 杀读端：断端前置位。
    pub fn kill_read_end(&mut self) -> Result<(), u32> {
        if self.frozen {
            return Err(KS_FROZEN);
        }
        if self.states.is_empty() {
            return Err(KS_EMPTY);
        }
        self.killed = true;
        Ok(())
    }

    /// 断端前写：读端未杀时写入必须成功（不落态）。
    pub fn write_before_kill(&mut self, no: usize) -> Result<u64, u32> {
        if self.frozen {
            return Err(KS_FROZEN);
        }
        let gix = self.group_ix(no)?;
        if !self.killed {
            // 读端还在：写入成功，不计入任何断端态
            self.groups[gix].writes = self.groups[gix].writes.wrapping_add(1);
            self.writes = self.writes.wrapping_add(1);
            return Ok(0);
        }
        Err(KS_LEAK)
    }

    /// 断端后写：按态落账。
    pub fn write_after_kill(&mut self, no: usize, kind: u8) -> Result<u8, u32> {
        if self.frozen {
            return Err(KS_FROZEN);
        }
        let gix = self.group_ix(no)?;
        if !self.killed {
            return Err(KS_LEAK);
        }
        let six = self.state_ix(kind)?;
        let s = &mut self.states[six];
        s.writes = s.writes.wrapping_add(1);
        let g = &mut self.groups[gix];
        g.writes = g.writes.wrapping_add(1);
        self.writes = self.writes.wrapping_add(1);
        if kind == KIND_SIGPIPE {
            s.sigpipes = s.sigpipes.wrapping_add(1);
            g.sigpipes = g.sigpipes.wrapping_add(1);
            self.sigpipes = self.sigpipes.wrapping_add(1);
        } else {
            s.epipes = s.epipes.wrapping_add(1);
            s.survived = s.survived.wrapping_add(1);
            g.epipes = g.epipes.wrapping_add(1);
            self.epipes = self.epipes.wrapping_add(1);
        }
        Ok(s.writes as u8)
    }

    /// 一轮：杀读端后三组各由三名写者逐人写一次（同态）。
    pub fn round(&mut self, r: u64) -> Result<u64, u32> {
        if !self.killed {
            self.kill_read_end()?;
        }
        let kind = Self::kind_at(r);
        let mut g = 0usize;
        while g < GROUPS {
            let mut w = 0u64;
            while w < WRITERS_PER_GROUP {
                self.write_after_kill(g, kind)?;
                w = w.wrapping_add(1);
            }
            g += 1;
        }
        self.rounds = self.rounds.wrapping_add(1);
        Ok(self.rounds)
    }

    /// 跑 n 轮。
    pub fn run(&mut self, n: u64) -> Result<u64, u32> {
        let mut k = 0u64;
        while k < n {
            self.round(k)?;
            k = k.wrapping_add(1);
        }
        Ok(self.rounds)
    }

    /// 三态对平：态账与总账逐项相等。
    pub fn states_balanced(&self) -> bool {
        let mut w = 0u64;
        let mut e = 0u64;
        let mut s = 0u64;
        let mut k = 0usize;
        while k < self.states.len() {
            let st = &self.states[k];
            if !st.balanced() || !st.total_ok() || !st.semantics() {
                return false;
            }
            w = w.wrapping_add(st.writes);
            e = e.wrapping_add(st.epipes);
            s = s.wrapping_add(st.sigpipes);
            k += 1;
        }
        w == self.writes && e == self.epipes && s == self.sigpipes
    }

    /// 组内隔离：组账之和等于总账，且逐组自平。
    pub fn isolation_gate(&self) -> Result<(), u32> {
        if self.groups.is_empty() || self.states.is_empty() {
            return Err(KS_EMPTY);
        }
        let mut w = 0u64;
        let mut e = 0u64;
        let mut s = 0u64;
        let mut k = 0usize;
        while k < self.groups.len() {
            let g = &self.groups[k];
            if !g.balanced() {
                return Err(KS_LEAK);
            }
            w = w.wrapping_add(g.writes);
            e = e.wrapping_add(g.epipes);
            s = s.wrapping_add(g.sigpipes);
            k += 1;
        }
        if w != self.writes || e != self.epipes || s != self.sigpipes {
            return Err(KS_LEAK);
        }
        Ok(())
    }

    /// 全链对平：三态之和 == 写次，且存亡之和 == 写次。
    pub fn chain_balanced(&self) -> bool {
        self.writes > 0
            && self.epipes.wrapping_add(self.sigpipes) == self.writes
            && self.states.len() == KINDS
            && self.groups.len() == GROUPS
    }

    /// 某态写次。
    pub fn writes_of(&self, kind: u8) -> Result<u64, u32> {
        let ix = self.state_ix(kind)?;
        Ok(self.states[ix].writes)
    }

    /// 链式 fold 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = KS_TAG;
        let mut k = 0usize;
        while k < self.states.len() {
            let s = &self.states[k];
            h = (h ^ s.kid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ s.writes).wrapping_mul(0x100000001b3);
            h = (h ^ s.epipes).wrapping_mul(0x100000001b3);
            h = (h ^ s.sigpipes).wrapping_mul(0x100000001b3);
            k += 1;
        }
        let mut j = 0usize;
        while j < self.groups.len() {
            let g = &self.groups[j];
            h = (h ^ g.gid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ g.writes).wrapping_mul(0x100000001b3);
            j += 1;
        }
        h
    }

    /// 冻结账。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：千轮三态全链对平且组内隔离。
pub fn sample_pressure() -> (KillBook, bool) {
    let mut b = KillBook::new();
    let _ = b.run(ROUNDS);
    let ok = b.chain_balanced() && b.states_balanced() && b.isolation_gate().is_ok();
    b.freeze();
    (b, ok)
}

/// 注入核：坏态号 / 越界组号 / 冻结后写 / 断端前写。
pub fn inject_four() -> [bool; 4] {
    let mut b = KillBook::new();
    let _ = b.kill_read_end();
    // 注入 1：坏态号 → KS_BADKIND 拒收
    let i1 = matches!(b.write_after_kill(0, 9), Err(KS_BADKIND));
    // 注入 2：越界组号 → KS_RANGE 拒收
    let i2 = matches!(b.write_after_kill(GROUPS, KIND_EPIPE), Err(KS_RANGE));
    // 注入 3：冻结后写 → KS_FROZEN 拒收
    b.freeze();
    let i3 = matches!(b.write_after_kill(0, KIND_EPIPE), Err(KS_FROZEN));
    // 注入 4：断端前写 → KS_LEAK 拒止
    let mut c = KillBook::new();
    let i4 = matches!(c.write_before_kill(0), Ok(_)) && matches!(c.write_after_kill(0, KIND_EPIPE), Err(KS_LEAK));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 千次三态对账，组内隔离对平。
pub fn run_unx10984_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10984");
    let (b, ok) = sample_pressure();

    s.add("J1-三态对账", ok && b.chain_balanced(), "千轮三态写次之和等于总写次");
    s.add("J1-组内隔离", b.isolation_gate().is_ok(), "三组账互不借贷且逐组自平");
    s.add("J2-态账对平", b.states_balanced(), "三态账与总账逐项相等");
    s.add("J2-存亡守恒", b.epipes.wrapping_add(b.sigpipes) == b.writes, "返错加倒下恰等于写次");
    s.add("J2-写次守恒", b.writes == ROUNDS * GROUPS as u64 * WRITERS_PER_GROUP, "千轮三组三写次守恒");
    s.add("J3-态分布", b.writes_of(KIND_EPIPE).unwrap_or(0) + b.writes_of(KIND_SIGPIPE).unwrap_or(0) + b.writes_of(KIND_IGNORED).unwrap_or(0) == b.writes, "三态写次之和等于总写次");
    s.add("J3-分布均匀", {
        let a = b.writes_of(KIND_EPIPE).unwrap_or(0);
        let m = b.writes_of(KIND_SIGPIPE).unwrap_or(0);
        let o = b.writes_of(KIND_IGNORED).unwrap_or(0);
        let mx = if a > m { if a > o { a } else { o } } else if m > o { m } else { o };
        let mn = if a < m { if a < o { a } else { o } } else if m < o { m } else { o };
        // 每轮只把一轮的三组三写（9 笔）记到同一个态，1000 轮按三态轮转
        // 必然余 1 轮（1000 % 3 == 1），故极差恰为一整轮 9 笔而非 3 笔。
        mx.wrapping_sub(mn) <= (GROUPS as u64).wrapping_mul(WRITERS_PER_GROUP)
    }, "三态轮转分布差不超过一轮三组三写");
    s.add("J4-存活语义", b.states.iter().all(|x| x.semantics()), "EPIPE/忽略态全存活，SIGPIPE 态全倒下");
    s.add("J4-组均沾", b.groups.iter().all(|g| g.writes == ROUNDS * WRITERS_PER_GROUP), "三组写次完全均等");
    s.add("J4-组存亡", b.groups.iter().all(|g| !g.all_down()), "三组均有存活不整组倒下");
    s.add("J5-断端前置", {
        let mut g = KillBook::new();
        let before = g.write_before_kill(0);
        before == Ok(0) && g.killed == false && g.epipes == 0 && g.sigpipes == 0
    }, "断端前写入成功且不落断端态");
    s.add("J6-坏态拒写", inject_four()[0], "坏态号被 KS_BADKIND 拒收");
    s.add("J6-越界拒写", inject_four()[1], "越界组号被 KS_RANGE 拒收");
    s.add("J7-冻结拒写", inject_four()[2], "冻结后写被 KS_FROZEN 拒止");
    s.add("J7-断端前拒落态", inject_four()[3], "断端前落断端态被 KS_LEAK 拒止");
    s.add("J8-多立态即失衡", {
        let mut d = KillBook::new();
        let _ = d.run(10);
        let before = d.states_balanced();
        d.states.push(KillState {
            kid: 0x15900,
            kind: KIND_EPIPE,
            name: "dup".to_string(),
            writes: 1,
            epipes: 1,
            sigpipes: 0,
            survived: 1,
        });
        before && !d.states_balanced()
    }, "多立一条态账即态账与总账失衡");
    s.add("J8-指纹稳定", b.frozen && b.fold() == b.fold(), "冻结后指纹重算稳定");
    s.add_dyn(
        "J9-断端动态",
        ok,
        &format!(
            "轮 {} 写 {} 返错 {} 倒下 {} EPIPE/SIGPIPE/IGNORED={}/{}/{}",
            b.rounds,
            b.writes,
            b.epipes,
            b.sigpipes,
            b.writes_of(KIND_EPIPE).unwrap_or(0),
            b.writes_of(KIND_SIGPIPE).unwrap_or(0),
            b.writes_of(KIND_IGNORED).unwrap_or(0),
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10984_kill_pressure_flow() {
        let mut b = KillBook::new();
        assert_eq!(b.run(ROUNDS).unwrap_or(0), ROUNDS);
        assert!(b.chain_balanced());
        assert!(b.states_balanced());
        assert!(b.isolation_gate().is_ok());
        assert_eq!(b.writes, ROUNDS * GROUPS as u64 * WRITERS_PER_GROUP);
        // 单轮语义抽查
        let mut g = KillBook::new();
        let _ = g.kill_read_end();
        assert_eq!(g.write_after_kill(0, KIND_SIGPIPE).unwrap_or(0), 1);
        assert_eq!(g.sigpipes, 1);
        assert_eq!(g.epipes, 0);
        assert_eq!(KillBook::kind_at(0), KIND_EPIPE);
        assert_eq!(KillBook::kind_at(1), KIND_SIGPIPE);
        assert_eq!(KillBook::kind_at(2), KIND_IGNORED);
        let root = b.freeze();
        assert!(b.frozen);
        assert_eq!(root, b.fold());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx10984_checks_green() {
        let s = run_unx10984_checks();
        assert!(s.all_passed(), "UNX-F10984 判据未全绿");
    }
}
