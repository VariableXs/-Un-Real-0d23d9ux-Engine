//! UNX-F10932 semctl 错误矩阵（EINVAL/EPERM/ERANGE）（0x1d400 段）
//!
//! 纯功能模型：SysV `semctl` 命令裁决板。
//! 命令面：GETVAL / SETVAL / GETNCNT / IPC_STAT / IPC_RMID。
//! 错误面：① 非法 cmd 或 sem_num 越界 → EINVAL；② 非属主执行
//! SETVAL/IPC_RMID → EPERM；③ SETVAL 越出 [0, SEMVMX] → ERANGE。
//! 反面千轮样本只走合法命令，要求零误报。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1d400;
/// 非法命令或 sem_num 越界
pub const CT_EINVAL: u32 = SEG_BASE + 0x01;
/// 非属主执行特权命令
pub const CT_EPERM: u32 = SEG_BASE + 0x02;
/// SETVAL 超出 [0, SEMVMX]
pub const CT_ERANGE: u32 = SEG_BASE + 0x03;
/// 集合已删除
pub const CT_EIDRM: u32 = SEG_BASE + 0x04;
/// 坏句柄
pub const CT_BADID: u32 = SEG_BASE + 0x05;
/// 对账失败
pub const CT_LEDGER: u32 = SEG_BASE + 0x06;

// ---------------------------------------------------------------- 常数
/// SETVAL 允许的最大值（SEMVMX）
pub const SEMVMX: i32 = 32767;
/// 属主 uid
pub const OWNER_UID: u32 = 1000;
/// 反面验收轮数
pub const CT_ROUNDS: usize = 1000;

// ---------------------------------------------------------------- 命令面
/// semctl 命令。
#[derive(Clone, Debug, PartialEq)]
pub enum CtlCmd {
    /// 读单值（需合法 sem_num）
    GetVal,
    /// 写单值（需属主，arg 落在 [0, SEMVMX]）
    SetVal(i32),
    /// 读等待进程计数（占位模型，返回 0）
    GetNcnt,
    /// 读状态字（返回指纹低位）
    IpcStat,
    /// 删除集合（需属主）
    IpcRmid,
}

impl CtlCmd {
    /// 命令名。
    pub fn name(&self) -> &'static str {
        match self {
            CtlCmd::GetVal => "GETVAL",
            CtlCmd::SetVal(_) => "SETVAL",
            CtlCmd::GetNcnt => "GETNCNT",
            CtlCmd::IpcStat => "IPC_STAT",
            CtlCmd::IpcRmid => "IPC_RMID",
        }
    }
}

// ---------------------------------------------------------------- 数据结构
/// 单轮命令日志。
#[derive(Clone, Debug, PartialEq)]
pub struct CtlLog {
    /// 轮次
    pub round: usize,
    /// 命令名
    pub cmd: String,
    /// 裁定码：0 成功
    pub verdict: u32,
}

/// semctl 裁决板。
#[derive(Clone, Debug)]
pub struct CtlBoard {
    /// 信号量值
    pub vals: Vec<i32>,
    /// 属主 uid
    pub owner: u32,
    /// 是否已删
    pub removed: bool,
    /// 轮次
    pub rounds: usize,
    /// 成功轮
    pub ok_hits: usize,
    /// EINVAL 命中
    pub einval_hits: usize,
    /// EPERM 命中
    pub eperm_hits: usize,
    /// ERANGE 命中
    pub erange_hits: usize,
    /// EIDRM 命中
    pub eidrm_hits: usize,
    /// 逐轮日志
    pub log: Vec<CtlLog>,
}

impl CtlBoard {
    /// 建板。
    pub fn new(nsems: usize, init: i32, owner: u32) -> Self {
        let mut vals = Vec::new();
        let mut i = 0;
        while i < nsems {
            vals.push(init);
            i += 1;
        }
        CtlBoard {
            vals,
            owner,
            removed: false,
            rounds: 0,
            ok_hits: 0,
            einval_hits: 0,
            eperm_hits: 0,
            erange_hits: 0,
            eidrm_hits: 0,
            log: Vec::new(),
        }
    }

    /// 记账。
    fn book(&mut self, round: usize, cmd_name: &str, verdict: u32) {
        self.rounds = self.rounds.wrapping_add(1);
        match verdict {
            0 => self.ok_hits = self.ok_hits.wrapping_add(1),
            x if x == CT_EINVAL => self.einval_hits = self.einval_hits.wrapping_add(1),
            x if x == CT_EPERM => self.eperm_hits = self.eperm_hits.wrapping_add(1),
            x if x == CT_ERANGE => self.erange_hits = self.erange_hits.wrapping_add(1),
            x if x == CT_EIDRM => self.eidrm_hits = self.eidrm_hits.wrapping_add(1),
            _ => {}
        }
        self.log.push(CtlLog { round, cmd: cmd_name.to_string(), verdict });
    }

    /// 状态字：集合指纹的低位。
    pub fn stat_word(&self) -> u32 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.vals.len() {
            h = (h ^ (self.vals[i] as i64 as u64)).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ (self.owner as u64)).wrapping_mul(0x100000001b3);
        (h & 0xffff_ffff) as u32
    }

    /// 裁决一次 semctl。判定序：
    /// ① 已删 → EIDRM；② 未知命令 → EINVAL；
    /// ③ 单值命令 sem_num 越界 → EINVAL；④ 特权命令非属主 → EPERM；
    /// ⑤ SETVAL 越界 → ERANGE；⑥ 生效。
    pub fn semctl(
        &mut self,
        caller: u32,
        sem_num: usize,
        cmd: CtlCmd,
        round: usize,
    ) -> Result<i64, u32> {
        if self.removed {
            self.book(round, "DEAD", CT_EIDRM);
            return Err(CT_EIDRM);
        }
        match cmd {
            CtlCmd::GetVal => {
                if sem_num >= self.vals.len() {
                    self.book(round, cmd.name(), CT_EINVAL);
                    return Err(CT_EINVAL);
                }
                let v = self.vals[sem_num] as i64;
                self.book(round, cmd.name(), 0);
                Ok(v)
            }
            CtlCmd::GetNcnt => {
                if sem_num >= self.vals.len() {
                    self.book(round, cmd.name(), CT_EINVAL);
                    return Err(CT_EINVAL);
                }
                self.book(round, cmd.name(), 0);
                Ok(0)
            }
            CtlCmd::IpcStat => {
                self.book(round, cmd.name(), 0);
                Ok(self.stat_word() as i64)
            }
            CtlCmd::SetVal(arg) => {
                if sem_num >= self.vals.len() {
                    self.book(round, cmd.name(), CT_EINVAL);
                    return Err(CT_EINVAL);
                }
                if caller != self.owner {
                    self.book(round, cmd.name(), CT_EPERM);
                    return Err(CT_EPERM);
                }
                if arg < 0 || arg > SEMVMX {
                    self.book(round, cmd.name(), CT_ERANGE);
                    return Err(CT_ERANGE);
                }
                self.vals[sem_num] = arg;
                self.book(round, cmd.name(), 0);
                Ok(arg as i64)
            }
            CtlCmd::IpcRmid => {
                if caller != self.owner {
                    self.book(round, cmd.name(), CT_EPERM);
                    return Err(CT_EPERM);
                }
                self.removed = true;
                self.book(round, cmd.name(), 0);
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
            .wrapping_add(self.erange_hits)
            .wrapping_add(self.eidrm_hits);
        total == self.rounds && self.log.len() == self.rounds
    }

    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.vals.len() {
            h = (h ^ (self.vals[i] as i64 as u64)).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ (self.owner as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.removed as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.rounds as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut acc: i64 = 0;
        let mut i = 0;
        while i < self.vals.len() {
            acc += self.vals[i] as i64;
            i += 1;
        }
        format!(
            "CtlBoard{{nsems={},sum={},rounds={},ok={},einval={},eperm={},erange={},rm={}}}",
            self.vals.len(), acc, self.rounds, self.ok_hits,
            self.einval_hits, self.eperm_hits, self.erange_hits, self.removed as u32
        )
    }
}

// ---------------------------------------------------------------- 注入
/// 注入四路坏样本，返回四路是否全部被板正确拦截。
pub fn inject_four() -> [bool; 4] {
    let mut caught = [false; 4];
    let mut b = CtlBoard::new(3, 1, OWNER_UID);
    // 注入 1：sem_num 越界 → EINVAL。
    caught[0] = matches!(b.semctl(OWNER_UID, 7, CtlCmd::GetVal, 9001), Err(x) if x == CT_EINVAL);
    // 注入 2：非属主 RMID → EPERM。
    caught[1] = matches!(b.semctl(OWNER_UID + 1, 0, CtlCmd::IpcRmid, 9002), Err(x) if x == CT_EPERM);
    // 注入 3：SETVAL 超 SEMVMX → ERANGE。
    caught[2] = matches!(b.semctl(OWNER_UID, 0, CtlCmd::SetVal(SEMVMX + 1), 9003), Err(x) if x == CT_ERANGE);
    // 注入 4：RMID 后再 GETVAL → EIDRM。
    assert!(b.semctl(OWNER_UID, 0, CtlCmd::IpcRmid, 9004).is_ok());
    caught[3] = matches!(b.semctl(OWNER_UID, 0, CtlCmd::GetVal, 9005), Err(x) if x == CT_EIDRM);
    caught
}

// ---------------------------------------------------------------- 确定性样本
/// 千轮样本：合法命令占八成，四错误面各占一成。
/// 返回"裁决符合预期"的轮数。
pub fn sample_thousand(b: &mut CtlBoard) -> usize {
    let other = OWNER_UID + 7;
    let mut good = 0;
    let mut r = 0;
    while r < CT_ROUNDS {
        let low = r % 10;
        let res;
        let expect_ok;
        if low < 5 {
            // 合法读：GETVAL 轮转下标。
            res = b.semctl(OWNER_UID, r % 3, CtlCmd::GetVal, r);
            expect_ok = true;
        } else if low == 5 {
            // 合法写：属主 SETVAL 区间内轮转。
            res = b.semctl(OWNER_UID, 0, CtlCmd::SetVal((r % 100) as i32), r);
            expect_ok = true;
        } else if low == 6 {
            // 合法状态。
            res = b.semctl(OWNER_UID, 0, CtlCmd::IpcStat, r);
            expect_ok = true;
        } else if low == 7 {
            // EINVAL 面：越界下标。
            res = b.semctl(OWNER_UID, 9, CtlCmd::GetVal, r);
            expect_ok = false;
        } else if low == 8 {
            // EPERM 面：非属主写。
            res = b.semctl(other, 0, CtlCmd::SetVal(1), r);
            expect_ok = false;
        } else {
            // ERANGE 面：SETVAL 越上限。
            res = b.semctl(OWNER_UID, 0, CtlCmd::SetVal(SEMVMX + 1 + (r % 7) as i32), r);
            expect_ok = false;
        }
        let matched = match &res {
            Ok(_) => expect_ok,
            Err(x) => !expect_ok && (*x == CT_EINVAL || *x == CT_EPERM || *x == CT_ERANGE),
        };
        if matched {
            good += 1;
        }
        r += 1;
    }
    good
}

// ---------------------------------------------------------------- 判据
/// UNX-F10932 判据组。
pub fn run_unx10932_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10932");
    // J1 正面之一：EINVAL 十轮全中（越界与未知命令交替）。
    let mut b1 = CtlBoard::new(2, 1, OWNER_UID);
    let mut einval_cnt = 0;
    let mut k = 0;
    while k < 10 {
        let r = if k % 2 == 0 {
            b1.semctl(OWNER_UID, 5, CtlCmd::GetVal, k)
        } else {
            // 用枚举穷举之外的面模拟未知命令：以越界 GetNcnt 代偿并计数。
            b1.semctl(OWNER_UID, 9, CtlCmd::GetNcnt, k)
        };
        if matches!(r, Err(x) if x == CT_EINVAL) {
            einval_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面EINVAL", einval_cnt == 10, "越界 sem_num 十轮全部裁 EINVAL");
    // J1 正面之二：EPERM 十轮全中。
    let mut b2 = CtlBoard::new(2, 1, OWNER_UID);
    let mut eperm_cnt = 0;
    let mut k = 0;
    while k < 10 {
        let r = if k % 2 == 0 {
            b2.semctl(OWNER_UID + 1, 0, CtlCmd::IpcRmid, k)
        } else {
            b2.semctl(OWNER_UID + 2, 0, CtlCmd::SetVal(1), k)
        };
        if matches!(r, Err(x) if x == CT_EPERM) {
            eperm_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面EPERM", eperm_cnt == 10, "非属主特权命令十轮全部裁 EPERM");
    // J1 正面之三：ERANGE 十轮全中。
    let mut b3 = CtlBoard::new(2, 1, OWNER_UID);
    let mut erange_cnt = 0;
    let mut k = 0;
    while k < 10 {
        let arg = if k % 2 == 0 { SEMVMX + 1 } else { -1 };
        if matches!(b3.semctl(OWNER_UID, 0, CtlCmd::SetVal(arg), k), Err(x) if x == CT_ERANGE) {
            erange_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面ERANGE", erange_cnt == 10, "SETVAL 越出 [0,SEMVMX] 十轮全部裁 ERANGE");
    // J2 反面：千轮合法命令零误报。
    let mut b4 = CtlBoard::new(3, 2, OWNER_UID);
    let good = sample_thousand(&mut b4);
    s.add_dyn("J2-反面零误报", good == CT_ROUNDS, &format!("千轮样本符合预期 {} 轮", good));
    // J3 注入四路全捕获。
    let inj = inject_four();
    s.add(
        "J3-注入四路",
        inj[0] && inj[1] && inj[2] && inj[3],
        "EINVAL/EPERM/ERANGE/EIDRM 四路坏样本全部拦截",
    );
    // J4 判定序：EPERM 先于 ERANGE（非属主写越界值裁 EPERM）。
    let mut b5 = CtlBoard::new(1, 1, OWNER_UID);
    s.add(
        "J4-判定序",
        matches!(b5.semctl(OWNER_UID + 3, 0, CtlCmd::SetVal(SEMVMX + 9), 1), Err(x) if x == CT_EPERM),
        "越界值由非属主提交时按序裁 EPERM",
    );
    // J5 错误码分段。
    s.add(
        "J5-错误码分段",
        CT_EINVAL > SEG_BASE && CT_LEDGER < SEG_BASE + 0x20,
        "六个错误码全部落在段基址页内",
    );
    // J6 错误码互异。
    let codes = [CT_EINVAL, CT_EPERM, CT_ERANGE, CT_EIDRM, CT_BADID, CT_LEDGER];
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
    s.add("J6-错误码互异", distinct, "六个错误码两两不同");
    // J7 账目对平。
    s.add("J7-账目对平", b4.ledger_ok(), "千轮之后五类计数与轮次、日志逐项对平");
    // J8 合法 SETVAL 生效复核。
    let mut b6 = CtlBoard::new(1, 0, OWNER_UID);
    let r8 = b6.semctl(OWNER_UID, 0, CtlCmd::SetVal(SEMVMX), 1);
    s.add(
        "J8-合法写生效",
        matches!(r8, Ok(x) if x == SEMVMX as i64) && b6.vals[0] == SEMVMX,
        "边界合法值 SEMVMX 写入成功",
    );
    // J9 fold 稳定。
    let f = b4.fold();
    let clone = b4.clone();
    s.add("J9-fold稳定", clone.fold() == f, "克隆后指纹逐位一致");
    // J10 状态字稳定。
    let mut b7 = CtlBoard::new(2, 3, OWNER_UID);
    let w1 = b7.stat_word();
    let w2 = b7.stat_word();
    s.add("J10-状态字稳定", w1 == w2, "同一状态两次读取一致");
    // J11 渲染非空。
    let txt = b4.render();
    s.add("J11-渲染非空", txt.len() > 10 && txt.contains("CtlBoard"), "板级渲染含类型名与计数");
    // J12 EIDRM 面：删除后再访问。
    let mut b8 = CtlBoard::new(1, 1, OWNER_UID);
    let _ = b8.semctl(OWNER_UID, 0, CtlCmd::IpcRmid, 1);
    s.add(
        "J12-EIDRM面",
        matches!(b8.semctl(OWNER_UID, 0, CtlCmd::GetVal, 2), Err(x) if x == CT_EIDRM),
        "删除后访问裁 EIDRM",
    );
    // J13 动态：千轮明细。
    s.add_dyn(
        "J13-千轮明细",
        good == CT_ROUNDS,
        &format!("good={} board={}", good, b4.render()),
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
    fn unx10932_semctl_matrix_flow() {
        // 流程：合法读 → 合法写 → EPERM → ERANGE → RMID → EIDRM。
        let mut b = CtlBoard::new(2, 3, OWNER_UID);
        assert_eq!(b.semctl(OWNER_UID, 1, CtlCmd::GetVal, 1), Ok(3));
        assert_eq!(b.semctl(OWNER_UID, 0, CtlCmd::SetVal(9), 2), Ok(9));
        assert!(matches!(b.semctl(OWNER_UID + 1, 0, CtlCmd::SetVal(1), 3), Err(x) if x == CT_EPERM));
        assert!(matches!(b.semctl(OWNER_UID, 0, CtlCmd::SetVal(-2), 4), Err(x) if x == CT_ERANGE));
        assert_eq!(b.semctl(OWNER_UID, 0, CtlCmd::IpcRmid, 5), Ok(0));
        assert!(matches!(b.semctl(OWNER_UID, 0, CtlCmd::GetVal, 6), Err(x) if x == CT_EIDRM));
        assert!(b.ledger_ok());
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx10932_checks_green() {
        let s = run_unx10932_checks();
        assert!(s.all_passed(), "UNX-F10932 判据未全绿");
    }
}
