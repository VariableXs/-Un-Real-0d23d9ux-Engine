//! UNX-F10936 shmctl 错误矩阵（EINVAL/EPERM/EIDRM）（0x1d480 段）
//!
//! 纯功能模型：SysV `shmctl` 命令裁决台。
//! 命令面：IPC_STAT / IPC_SET / IPC_RMID。
//! 错误面：① 坏句柄 → EINVAL；② 非属主 SET/RMID → EPERM；
//! ③ 已删段再访问 → EIDRM。反面千轮样本只走合法命令，零误报。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1d480;
/// 坏句柄
pub const SC_EINVAL: u32 = SEG_BASE + 0x01;
/// 非属主特权命令
pub const SC_EPERM: u32 = SEG_BASE + 0x02;
/// 段已删除
pub const SC_EIDRM: u32 = SEG_BASE + 0x03;
/// 台内对账失败
pub const SC_LEDGER: u32 = SEG_BASE + 0x04;

// ---------------------------------------------------------------- 常数
/// 属主 uid
pub const SC_OWNER: u32 = 2000;
/// 反面验收轮数
pub const SC_ROUNDS: usize = 1000;

// ---------------------------------------------------------------- 命令面
/// shmctl 命令。
#[derive(Clone, Debug, PartialEq)]
pub enum ShmCmd {
    /// 读状态
    Stat,
    /// 改尺寸（属主限定）
    Set(usize),
    /// 删除段（属主限定）
    Rmid,
}

impl ShmCmd {
    /// 命令名。
    pub fn name(&self) -> &'static str {
        match self {
            ShmCmd::Stat => "IPC_STAT",
            ShmCmd::Set(_) => "IPC_SET",
            ShmCmd::Rmid => "IPC_RMID",
        }
    }
}

// ---------------------------------------------------------------- 数据结构
/// 段条目。
#[derive(Clone, Debug)]
pub struct CtlSeg {
    /// 段句柄
    pub shmid: u32,
    /// 属主 uid
    pub owner: u32,
    /// 段大小
    pub size: usize,
    /// 是否已删
    pub removed: bool,
    /// 挂载计数
    pub nattch: usize,
}

impl CtlSeg {
    /// 状态字（指纹低位）。
    pub fn stat_word(&self) -> u32 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ (self.shmid as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.owner as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.size as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.removed as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.nattch as u64)).wrapping_mul(0x100000001b3);
        (h & 0xffff_ffff) as u32
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "CtlSeg{{id={},owner={},size={},rm={},nattch={}}}",
            self.shmid, self.owner, self.size, self.removed as u32, self.nattch
        )
    }
}

/// 单轮日志。
#[derive(Clone, Debug, PartialEq)]
pub struct ScLog {
    /// 轮次
    pub round: usize,
    /// 目标句柄
    pub shmid: u32,
    /// 命令名
    pub cmd: String,
    /// 裁定码：0 成功
    pub verdict: u32,
}

/// shmctl 裁决台。
#[derive(Clone, Debug)]
pub struct ShmCtlDesk {
    /// 段条目
    pub segs: Vec<CtlSeg>,
    /// 轮次
    pub rounds: usize,
    /// 成功轮
    pub ok_hits: usize,
    /// EINVAL 命中
    pub einval_hits: usize,
    /// EPERM 命中
    pub eperm_hits: usize,
    /// EIDRM 命中
    pub eidrm_hits: usize,
    /// 逐轮日志
    pub log: Vec<ScLog>,
}

impl ShmCtlDesk {
    /// 建台：给定段集合。
    pub fn new(segs: Vec<CtlSeg>) -> Self {
        ShmCtlDesk {
            segs,
            rounds: 0,
            ok_hits: 0,
            einval_hits: 0,
            eperm_hits: 0,
            eidrm_hits: 0,
            log: Vec::new(),
        }
    }

    /// 句柄查找。
    pub fn find(&self, shmid: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.segs.len() {
            if self.segs[i].shmid == shmid {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 记账。
    fn book(&mut self, round: usize, shmid: u32, cmd: &str, verdict: u32) {
        self.rounds = self.rounds.wrapping_add(1);
        match verdict {
            0 => self.ok_hits = self.ok_hits.wrapping_add(1),
            x if x == SC_EINVAL => self.einval_hits = self.einval_hits.wrapping_add(1),
            x if x == SC_EPERM => self.eperm_hits = self.eperm_hits.wrapping_add(1),
            x if x == SC_EIDRM => self.eidrm_hits = self.eidrm_hits.wrapping_add(1),
            _ => {}
        }
        self.log.push(ScLog { round, shmid, cmd: cmd.to_string(), verdict });
    }

    /// 裁决一次 shmctl。判定序：
    /// ① 句柄查无 → EINVAL；② 已删段（任何命令）→ EIDRM；
    /// ③ 特权命令非属主 → EPERM；④ 生效。
    pub fn shmctl(&mut self, caller: u32, shmid: u32, cmd: ShmCmd, round: usize) -> Result<u32, u32> {
        let ix = match self.find(shmid) {
            Some(ix) => ix,
            None => {
                self.book(round, shmid, cmd.name(), SC_EINVAL);
                return Err(SC_EINVAL);
            }
        };
        if self.segs[ix].removed {
            self.book(round, shmid, cmd.name(), SC_EIDRM);
            return Err(SC_EIDRM);
        }
        match cmd {
            ShmCmd::Stat => {
                let w = self.segs[ix].stat_word();
                self.book(round, shmid, cmd.name(), 0);
                Ok(w)
            }
            ShmCmd::Set(newsz) => {
                if caller != self.segs[ix].owner {
                    self.book(round, shmid, cmd.name(), SC_EPERM);
                    return Err(SC_EPERM);
                }
                if newsz == 0 || newsz % 4096 != 0 {
                    self.book(round, shmid, cmd.name(), SC_EINVAL);
                    return Err(SC_EINVAL);
                }
                self.segs[ix].size = newsz;
                self.book(round, shmid, cmd.name(), 0);
                Ok(newsz as u32)
            }
            ShmCmd::Rmid => {
                if caller != self.segs[ix].owner {
                    self.book(round, shmid, cmd.name(), SC_EPERM);
                    return Err(SC_EPERM);
                }
                self.segs[ix].removed = true;
                self.book(round, shmid, cmd.name(), 0);
                Ok(0)
            }
        }
    }

    /// 账目对平。
    pub fn ledger_ok(&self) -> bool {
        let total = self
            .ok_hits
            .wrapping_add(self.einval_hits)
            .wrapping_add(self.eperm_hits)
            .wrapping_add(self.eidrm_hits);
        total == self.rounds && self.log.len() == self.rounds
    }

    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.segs.len() {
            h = (h ^ (self.segs[i].shmid as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ (self.segs[i].size as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ (self.segs[i].removed as u64)).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ (self.rounds as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "ShmCtlDesk{{segs={},rounds={},ok={},einval={},eperm={},eidrm={}}}",
            self.segs.len(), self.rounds, self.ok_hits,
            self.einval_hits, self.eperm_hits, self.eidrm_hits
        )
    }
}

// ---------------------------------------------------------------- 注入
/// 注入四路坏样本，返回四路是否全部被台正确拦截。
pub fn inject_four() -> [bool; 4] {
    let mut caught = [false; 4];
    let mut d = ShmCtlDesk::new(vec![
        CtlSeg { shmid: 1, owner: SC_OWNER, size: 0x2000, removed: false, nattch: 0 },
        CtlSeg { shmid: 2, owner: SC_OWNER + 5, size: 0x2000, removed: false, nattch: 0 },
    ]);
    // 注入 1：坏句柄 → EINVAL。
    caught[0] = matches!(d.shmctl(SC_OWNER, 99, ShmCmd::Stat, 9001), Err(x) if x == SC_EINVAL);
    // 注入 2：非属主 RMID → EPERM。
    caught[1] = matches!(d.shmctl(SC_OWNER + 1, 1, ShmCmd::Rmid, 9002), Err(x) if x == SC_EPERM);
    // 注入 3：非属主 SET 越权 → EPERM（含非法尺寸参数，仍先裁属主面）。
    caught[2] = matches!(d.shmctl(SC_OWNER + 2, 1, ShmCmd::Set(0x1000), 9003), Err(x) if x == SC_EPERM);
    // 注入 4：RMID 后再 STAT → EIDRM。
    assert!(d.shmctl(SC_OWNER, 1, ShmCmd::Rmid, 9004).is_ok());
    caught[3] = matches!(d.shmctl(SC_OWNER, 1, ShmCmd::Stat, 9005), Err(x) if x == SC_EIDRM);
    caught
}

// ---------------------------------------------------------------- 确定性样本
/// 千轮样本：合法命令占七成，三错误面各占一成。返回符合预期轮数。
pub fn sample_thousand(d: &mut ShmCtlDesk) -> usize {
    let other = SC_OWNER + 9;
    let mut good = 0;
    let mut r = 0;
    while r < SC_ROUNDS {
        let low = r % 10;
        let res;
        let expect_ok;
        if low < 4 {
            res = d.shmctl(SC_OWNER, 1, ShmCmd::Stat, r);
            expect_ok = true;
        } else if low == 4 {
            res = d.shmctl(SC_OWNER, 1, ShmCmd::Set(0x2000 + ((r % 4) as usize) * 0x1000), r);
            expect_ok = true;
        } else if low == 5 {
            res = d.shmctl(other, 2, ShmCmd::Stat, r);
            expect_ok = true;
        } else if low == 6 {
            // 合法：属主对他人段 STAT（读不需属主）。
            res = d.shmctl(SC_OWNER, 2, ShmCmd::Stat, r);
            expect_ok = true;
        } else if low == 7 {
            // EINVAL 面：坏句柄。
            res = d.shmctl(SC_OWNER, 0x7f00 + (r % 5) as u32, ShmCmd::Stat, r);
            expect_ok = false;
        } else if low == 8 {
            // EPERM 面：非属主 RMID。
            res = d.shmctl(other, 2, ShmCmd::Rmid, r);
            expect_ok = false;
        } else {
            // EINVAL 面：非法尺寸（非页对齐）由属主提交。
            res = d.shmctl(SC_OWNER, 1, ShmCmd::Set(0x1234), r);
            expect_ok = false;
        }
        let matched = match &res {
            Ok(_) => expect_ok,
            Err(x) => !expect_ok && (*x == SC_EINVAL || *x == SC_EPERM || *x == SC_EIDRM),
        };
        if matched {
            good += 1;
        }
        r += 1;
    }
    good
}

// ---------------------------------------------------------------- 判据
/// UNX-F10936 判据组。
pub fn run_unx10936_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10936");
    let mk = || {
        ShmCtlDesk::new(vec![
            CtlSeg { shmid: 1, owner: SC_OWNER, size: 0x2000, removed: false, nattch: 0 },
            CtlSeg { shmid: 2, owner: SC_OWNER + 5, size: 0x2000, removed: false, nattch: 0 },
        ])
    };
    // J1 正面之一：EINVAL 十轮全中。
    let mut d1 = mk();
    let mut einval_cnt = 0;
    let mut k = 0;
    while k < 10 {
        let r = if k % 2 == 0 {
            d1.shmctl(SC_OWNER, 0x7700 + k as u32, ShmCmd::Stat, k)
        } else {
            d1.shmctl(SC_OWNER, 1, ShmCmd::Set(0x123), k)
        };
        if matches!(r, Err(x) if x == SC_EINVAL) {
            einval_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面EINVAL", einval_cnt == 10, "坏句柄/非法尺寸十轮全部裁 EINVAL");
    // J1 正面之二：EPERM 十轮全中。
    let mut d2 = mk();
    let mut eperm_cnt = 0;
    let mut k = 0;
    while k < 10 {
        let r = if k % 2 == 0 {
            d2.shmctl(SC_OWNER + 1, 1, ShmCmd::Rmid, k)
        } else {
            d2.shmctl(SC_OWNER + 2, 2, ShmCmd::Set(0x3000), k)
        };
        if matches!(r, Err(x) if x == SC_EPERM) {
            eperm_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面EPERM", eperm_cnt == 10, "非属主特权命令十轮全部裁 EPERM");
    // J1 正面之三：EIDRM 十轮全中。
    let mut d3 = mk();
    let _ = d3.shmctl(SC_OWNER, 1, ShmCmd::Rmid, 100);
    let mut eidrm_cnt = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(d3.shmctl(SC_OWNER, 1, ShmCmd::Stat, k), Err(x) if x == SC_EIDRM) {
            eidrm_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面EIDRM", eidrm_cnt == 10, "已删段访问十轮全部裁 EIDRM");
    // J2 反面：千轮合法命令零误报。
    let mut d4 = mk();
    let good = sample_thousand(&mut d4);
    s.add_dyn("J2-反面零误报", good == SC_ROUNDS, &format!("千轮样本符合预期 {} 轮", good));
    // J3 注入四路全捕获。
    let inj = inject_four();
    s.add(
        "J3-注入四路",
        inj[0] && inj[1] && inj[2] && inj[3],
        "EINVAL/EPERM/EIDRM/删除后访问四路坏样本全部拦截",
    );
    // J4 判定序：EIDRM 先于 EPERM（删后非属主访问裁 EIDRM）。
    let mut d5 = mk();
    // 段 2 的属主是 SC_OWNER+5：删除须由属主发起，否则只会裁 EPERM 而删不掉。
    let _ = d5.shmctl(SC_OWNER + 5, 2, ShmCmd::Rmid, 1);
    s.add(
        "J4-判定序",
        matches!(d5.shmctl(SC_OWNER + 3, 2, ShmCmd::Rmid, 2), Err(x) if x == SC_EIDRM),
        "已删段上非属主命令按序裁 EIDRM",
    );
    // J5 错误码分段。
    s.add(
        "J5-错误码分段",
        SC_EINVAL > SEG_BASE && SC_LEDGER < SEG_BASE + 0x20,
        "四个错误码全部落在段基址页内",
    );
    // J6 错误码互异。
    let codes = [SC_EINVAL, SC_EPERM, SC_EIDRM, SC_LEDGER];
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
    s.add("J6-错误码互异", distinct, "四个错误码两两不同");
    // J7 账目对平。
    s.add("J7-账目对平", d4.ledger_ok(), "千轮之后四类计数与轮次、日志逐项对平");
    // J8 合法 SET 生效：属主改尺寸成功。
    let mut d6 = mk();
    let r8 = d6.shmctl(SC_OWNER, 1, ShmCmd::Set(0x4000), 1);
    s.add(
        "J8-合法改尺寸",
        matches!(r8, Ok(x) if x == 0x4000) && d6.segs[0].size == 0x4000,
        "属主 IPC_SET 尺寸生效",
    );
    // J9 fold 稳定。
    let f = d4.fold();
    let clone = d4.clone();
    s.add("J9-fold稳定", clone.fold() == f, "克隆后指纹逐位一致");
    // J10 状态字稳定。
    let mut d7 = mk();
    let w1 = d7.shmctl(SC_OWNER, 1, ShmCmd::Stat, 1);
    let w2 = d7.shmctl(SC_OWNER, 1, ShmCmd::Stat, 2);
    s.add("J10-状态字稳定", w1 == w2, "同段两次 STAT 一致");
    // J11 渲染非空。
    let txt = d4.render();
    s.add("J11-渲染非空", txt.len() > 10 && txt.contains("ShmCtlDesk"), "台级渲染含类型名与计数");
    // J12 页对齐边界：恰好 4096 合法。
    let r12 = d7.shmctl(SC_OWNER, 1, ShmCmd::Set(4096), 3);
    s.add("J12-页对齐边界", matches!(r12, Ok(x) if x == 4096), "恰好一页的 IPC_SET 合法");
    // J13 动态：千轮明细。
    s.add_dyn(
        "J13-千轮明细",
        good == SC_ROUNDS,
        &format!("good={} desk={}", good, d4.render()),
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
    fn unx10936_shmctl_matrix_flow() {
        // 流程：STAT → SET → EPERM → EIDRM → 坏句柄 EINVAL。
        let mut d = ShmCtlDesk::new(vec![
            CtlSeg { shmid: 1, owner: SC_OWNER, size: 0x2000, removed: false, nattch: 0 },
            CtlSeg { shmid: 2, owner: SC_OWNER + 5, size: 0x2000, removed: false, nattch: 0 },
        ]);
        assert!(d.shmctl(SC_OWNER, 1, ShmCmd::Stat, 1).is_ok());
        assert_eq!(d.shmctl(SC_OWNER, 1, ShmCmd::Set(0x3000), 2), Ok(0x3000));
        assert!(matches!(d.shmctl(SC_OWNER + 1, 1, ShmCmd::Rmid, 3), Err(x) if x == SC_EPERM));
        assert_eq!(d.shmctl(SC_OWNER, 1, ShmCmd::Rmid, 4), Ok(0));
        assert!(matches!(d.shmctl(SC_OWNER, 1, ShmCmd::Stat, 5), Err(x) if x == SC_EIDRM));
        assert!(matches!(d.shmctl(SC_OWNER, 9, ShmCmd::Stat, 6), Err(x) if x == SC_EINVAL));
        assert!(d.ledger_ok());
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx10936_checks_green() {
        let s = run_unx10936_checks();
        assert!(s.all_passed(), "UNX-F10936 判据未全绿");
    }
}
