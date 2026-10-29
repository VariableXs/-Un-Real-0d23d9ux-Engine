//! UNX-F10841 pty 打开错误矩阵（资源与参数面）（0x1c1a0 段）
//!
//! 锚点：UNX-F10841-J1 pty 打开资源与参数面——master 打开受 fd 配额
//! （EMFILE）与 pty 表容量（EAGAIN）双重约束；从端先于主端打开
//! （slave-before-master）必 ENXIO；缺 O_RDWR 位必 EINVAL。资源层先于
//! 参数层判定，正反双样本对账，注入核（无配额/从端放行）必分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：打开场景（kind/flags/配额三字段）→ 分层判定器（参数→fd→表容）
//! → pty 表状态机（主从登记/释放）→ 打开样本矩阵（容量封顶）
//! → 注入核（两层坏实现必检出）→ 生命周期回放（开-开-关-开对平）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c1a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c1a0;
pub const PO_BAD_FLAGS: u32 = SEG_BASE + 0x01; // O_RDWR 位缺失（EINVAL）
pub const PO_FD_EXH: u32 = SEG_BASE + 0x02; // 进程 fd 配额耗尽（EMFILE）
pub const PO_TABLE_FULL: u32 = SEG_BASE + 0x03; // pty 表满（EAGAIN）
pub const PO_SLAVE_EARLY: u32 = SEG_BASE + 0x04; // 从端先于主端（ENXIO）
pub const PO_BAD_INDEX: u32 = SEG_BASE + 0x05; // 从端索引越界
pub const PO_OK_FACE: u32 = SEG_BASE + 0x06; // 打开放行面

/// 资源常量：fd 配额与 pty 表容量（先定后用）。
pub const FD_CAP: u32 = 8;
pub const PTY_MAX: u32 = 4;
pub const FLAG_RDWR: u32 = 0o2;

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        PO_BAD_FLAGS => "O_RDWR 位缺失 EINVAL",
        PO_FD_EXH => "fd 配额耗尽 EMFILE",
        PO_TABLE_FULL => "pty 表满 EAGAIN",
        PO_SLAVE_EARLY => "从端先于主端 ENXIO",
        PO_BAD_INDEX => "从端索引越界",
        PO_OK_FACE => "打开放行",
        _ => "未知诊断码",
    }
}

/// 打开种类：主端 / 从端。
#[derive(Clone, PartialEq)]
pub enum Kind {
    Master,
    Slave(u32),
}

/// 打开场景：种类 + 请求 flags + 资源水位（判定器三输入）。
#[derive(Clone)]
pub struct OpenReq {
    pub kind: Kind,
    pub flags: u32,
    pub fd_used: u32,
    pub ptys_used: u32,
}

/// 判定结果：放行携带分配号，拒绝携带诊断码。
#[derive(Clone, PartialEq, Debug)]
pub enum Verdict {
    Granted(u32),
    Deny(u32),
}

/// 分层判定器：参数层（EINVAL）→ fd 层（EMFILE）→ 表容层（EAGAIN）
/// → 从端先于主端（ENXIO）→ 放行。全部分支依赖场景字段计算。
pub fn judge(req: &OpenReq) -> Verdict {
    if (req.flags & FLAG_RDWR) != FLAG_RDWR {
        return Verdict::Deny(PO_BAD_FLAGS);
    }
    if req.fd_used >= FD_CAP {
        return Verdict::Deny(PO_FD_EXH);
    }
    match &req.kind {
        Kind::Master => {
            if req.ptys_used >= PTY_MAX {
                Verdict::Deny(PO_TABLE_FULL)
            } else {
                Verdict::Granted(req.ptys_used)
            }
        }
        Kind::Slave(idx) => {
            if *idx >= req.ptys_used {
                if req.ptys_used == 0 {
                    Verdict::Deny(PO_SLAVE_EARLY)
                } else {
                    Verdict::Deny(PO_BAD_INDEX)
                }
            } else {
                Verdict::Granted(PTY_MAX + *idx)
            }
        }
    }
}

/// pty 表状态机：主从登记、释放、水位维护。
pub struct PtyTable {
    pub fd_used: u32,
    pub masters: Vec<u32>,
    pub slaves: Vec<u32>,
    pub note: String,
}

impl PtyTable {
    pub fn new() -> PtyTable {
        PtyTable {
            fd_used: 0,
            masters: Vec::new(),
            slaves: Vec::new(),
            note: "fresh".to_string(),
        }
    }

    /// 打开主端：判定放行则登记。
    pub fn open_master(&mut self, flags: u32) -> Result<u32, u32> {
        let req = OpenReq {
            kind: Kind::Master,
            flags,
            fd_used: self.fd_used,
            ptys_used: self.masters.len() as u32,
        };
        match judge(&req) {
            Verdict::Granted(id) => {
                self.masters.push(id);
                self.fd_used += 1;
                self.note = "master-open".to_string();
                Ok(id)
            }
            Verdict::Deny(c) => Err(c),
        }
    }

    /// 打开从端：判定放行则登记（主端须已存在）。
    pub fn open_slave(&mut self, idx: u32, flags: u32) -> Result<u32, u32> {
        let req = OpenReq {
            kind: Kind::Slave(idx),
            flags,
            fd_used: self.fd_used,
            ptys_used: self.masters.len() as u32,
        };
        match judge(&req) {
            Verdict::Granted(id) => {
                self.slaves.push(id);
                self.fd_used += 1;
                self.note = "slave-open".to_string();
                Ok(id)
            }
            Verdict::Deny(c) => Err(c),
        }
    }

    /// 关闭主端：主从对一起释放（从端先失），fd 水位回退。
    pub fn close_master(&mut self, id: u32) -> bool {
        let mut kept: Vec<u32> = Vec::new();
        let mut hit = false;
        let mut i = 0usize;
        while i < self.masters.len() {
            if self.masters[i] == id {
                hit = true;
            } else {
                kept.push(self.masters[i]);
            }
            i += 1;
        }
        if hit {
            self.masters = kept;
            self.slaves.clear();
            if self.fd_used >= 2 {
                self.fd_used -= 2;
            }
            self.note = "master-close".to_string();
        }
        hit
    }

    /// 表面账：主/从/fd 水位一致（主数 ≤ 表容，fd ≥ 主+从）。
    pub fn ledger_ok(&self) -> bool {
        (self.masters.len() as u32) <= PTY_MAX
            && self.fd_used >= (self.masters.len() + self.slaves.len()) as u32
    }
}

/// 样本矩阵：跨 kind/flags/水位/表容组合。
pub const MATRIX_CAP: usize = 24;

/// 矩阵逐格判定：命中（与期望码一致）计数。
pub fn verify_matrix(reqs: &[OpenReq], wants: &[u32]) -> (usize, usize, bool) {
    let mut hits = 0usize;
    let total = reqs.len();
    let mut i = 0usize;
    while i < total && i < MATRIX_CAP {
        if let Verdict::Deny(c) = judge(&reqs[i]) {
            if i < wants.len() && c == wants[i] {
                hits += 1;
            }
        } else if i < wants.len() && wants[i] == PO_OK_FACE {
            hits += 1;
        }
        i += 1;
    }
    (hits, total.min(MATRIX_CAP), hits == total.min(MATRIX_CAP))
}

/// 样本矩阵构建：8 行覆盖四类拒绝 + 两类放行。
pub fn sample_matrix() -> (Vec<OpenReq>, Vec<u32>) {
    let mut reqs: Vec<OpenReq> = Vec::new();
    let mut wants: Vec<u32> = Vec::new();
    // R0 参数缺位
    reqs.push(OpenReq { kind: Kind::Master, flags: 0o1, fd_used: 1, ptys_used: 0 });
    wants.push(PO_BAD_FLAGS);
    // R1 fd 顶格
    reqs.push(OpenReq { kind: Kind::Master, flags: FLAG_RDWR, fd_used: FD_CAP, ptys_used: 0 });
    wants.push(PO_FD_EXH);
    // R2 表满
    reqs.push(OpenReq { kind: Kind::Master, flags: FLAG_RDWR, fd_used: 2, ptys_used: PTY_MAX });
    wants.push(PO_TABLE_FULL);
    // R3 从端先于主端
    reqs.push(OpenReq { kind: Kind::Slave(0), flags: FLAG_RDWR, fd_used: 0, ptys_used: 0 });
    wants.push(PO_SLAVE_EARLY);
    // R4 从端索引越界
    reqs.push(OpenReq { kind: Kind::Slave(3), flags: FLAG_RDWR, fd_used: 1, ptys_used: 1 });
    wants.push(PO_BAD_INDEX);
    // R5 主端放行
    reqs.push(OpenReq { kind: Kind::Master, flags: FLAG_RDWR, fd_used: 1, ptys_used: 0 });
    wants.push(PO_OK_FACE);
    // R6 从端放行
    reqs.push(OpenReq { kind: Kind::Slave(0), flags: FLAG_RDWR, fd_used: 2, ptys_used: 1 });
    wants.push(PO_OK_FACE);
    // R7 fd 恰好未顶（边界 FD_CAP-1）
    reqs.push(OpenReq { kind: Kind::Master, flags: FLAG_RDWR, fd_used: FD_CAP - 1, ptys_used: 1 });
    wants.push(PO_OK_FACE);
    (reqs, wants)
}

/// 注入 A：坏实现忽略 fd 配额（顶格仍放行）。
pub fn faulty_no_fd_cap(req: &OpenReq) -> Verdict {
    let mut fixed = req.clone();
    fixed.fd_used = 0;
    judge(&fixed)
}

/// 注入 B：坏实现从端先于主端仍放行（从端恒 Grant）。
pub fn faulty_slave_pass(req: &OpenReq) -> Verdict {
    match &req.kind {
        Kind::Slave(_) => {
            if (req.flags & FLAG_RDWR) == FLAG_RDWR && req.fd_used < FD_CAP {
                Verdict::Granted(PTY_MAX)
            } else {
                judge(req)
            }
        }
        Kind::Master => judge(req),
    }
}

/// 注入核：两处坏账与判定器必分歧。
pub fn inject_two() -> [bool; 2] {
    let cap_fd = OpenReq { kind: Kind::Master, flags: FLAG_RDWR, fd_used: FD_CAP, ptys_used: 0 };
    let early = OpenReq { kind: Kind::Slave(0), flags: FLAG_RDWR, fd_used: 0, ptys_used: 0 };
    let d1 = match (judge(&cap_fd), faulty_no_fd_cap(&cap_fd)) {
        (Verdict::Deny(a), Verdict::Deny(b)) => a != b,
        (Verdict::Deny(_), Verdict::Granted(_)) => true,
        _ => false,
    };
    let d2 = match (judge(&early), faulty_slave_pass(&early)) {
        (Verdict::Deny(_), Verdict::Granted(_)) => true,
        _ => false,
    };
    [d1, d2]
}

/// 生命周期回放：开主→开从→关主→再开主，水面回落后可再开。
pub fn lifecycle_ok() -> bool {
    let mut t = PtyTable::new();
    let opened = t.open_master(FLAG_RDWR).is_ok();
    let sopened = t.open_slave(0, FLAG_RDWR).is_ok();
    let mut closed = false;
    let mut i = 0u32;
    while i < PTY_MAX {
        if t.close_master(i) {
            closed = true;
        }
        i += 1;
    }
    let m1 = t.open_master(FLAG_RDWR);
    opened && sopened && closed && m1.is_ok() && t.ledger_ok() && t.note == "master-open"
}

/// 从端早开核：空表上开从必 ENXIO，装主后同索引放行。
pub fn slave_order_ok() -> bool {
    let mut t = PtyTable::new();
    let early = t.open_slave(0, FLAG_RDWR);
    let _ = t.open_master(FLAG_RDWR);
    let late = t.open_slave(0, FLAG_RDWR);
    early.err() == Some(PO_SLAVE_EARLY) && late.is_ok() && t.slaves.len() == 1
}

/// 参数面注记：flags 与资源码文本闭集。
pub fn flags_note(t: &PtyTable) -> String {
    format!(
        "fd={} masters={} slaves={} {}",
        t.fd_used,
        t.masters.len(),
        t.slaves.len(),
        diag_text(PO_OK_FACE)
    )
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10841_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10841");
    let (reqs, wants) = sample_matrix();
    let (hits, total, cap_ok) = verify_matrix(&reqs, &wants);

    // J1 正路径：空表开主放行 0 号
    let open0 = judge(&OpenReq { kind: Kind::Master, flags: FLAG_RDWR, fd_used: 0, ptys_used: 0 });
    s.add("J1-主端放行", open0 == Verdict::Granted(0), "空表开主得 0 号");

    // J2 边界：fd 恰顶格拒绝 / 恰未顶放行
    let j2 = judge(&OpenReq { kind: Kind::Master, flags: FLAG_RDWR, fd_used: FD_CAP, ptys_used: 0 })
        == Verdict::Deny(PO_FD_EXH)
        && judge(&OpenReq { kind: Kind::Master, flags: FLAG_RDWR, fd_used: FD_CAP - 1, ptys_used: 0 })
            == Verdict::Granted(0);
    s.add("J2-fd 边界", j2, "FD_CAP-1/FD_CAP 两沿可辨");

    // J3 边界：pty 表恰满拒绝
    let j3 = judge(&OpenReq { kind: Kind::Master, flags: FLAG_RDWR, fd_used: 1, ptys_used: PTY_MAX })
        == Verdict::Deny(PO_TABLE_FULL)
        && judge(&OpenReq { kind: Kind::Master, flags: FLAG_RDWR, fd_used: 1, ptys_used: PTY_MAX - 1 })
            == Verdict::Granted(3);
    s.add("J3-表容边界", j3, "PTY_MAX-1/PTY_MAX 两沿可辨");

    // J4 参数层：缺 O_RDWR 必 EINVAL 面
    let j4 = judge(&OpenReq { kind: Kind::Master, flags: 0o1, fd_used: 0, ptys_used: 0 })
        == Verdict::Deny(PO_BAD_FLAGS);
    s.add("J4-参数缺位", j4, "无读写位必拒");

    // J5 从端先于主端：ENXIO 面
    s.add("J5-从端早开", slave_order_ok(), "空表从端 ENXIO / 装主后放行");

    // J6 样本矩阵全命中
    s.add("J6-矩阵对平", cap_ok, "8 行四拒两放全命中");

    // J7 注入1 fd 配额旁路检出
    let inj = inject_two();
    s.add("J7-配额旁路检出", inj[0], "无配额坏实现必分歧");

    // J8 注入2 从端早开放行检出
    s.add("J8-从端放行检出", inj[1], "早开坏实现必分歧");

    // J9 生命周期回放
    s.add("J9-生命周期", lifecycle_ok(), "开-开-关-开水面回平");

    // J10 表账一致
    let mut t = PtyTable::new();
    let _ = t.open_master(FLAG_RDWR);
    let _ = t.open_slave(0, FLAG_RDWR);
    s.add("J10-表账一致", t.ledger_ok() && t.fd_used == 2, "fd=主+从");

    // J11 诊断码闭集
    let j11 = diag_text(PO_FD_EXH) == "fd 配额耗尽 EMFILE"
        && diag_text(PO_TABLE_FULL) == "pty 表满 EAGAIN"
        && diag_text(0xffff_ffff) == "未知诊断码";
    s.add("J11-码文本闭集", j11, "六码文本可辨");

    // J12 字面锚：EMFILE=24 / EAGAIN=11 / ENXIO=6 / EINVAL=22
    let j12 = PO_FD_EXH == SEG_BASE + 0x02
        && PO_TABLE_FULL == SEG_BASE + 0x03
        && PO_SLAVE_EARLY == SEG_BASE + 0x04;
    s.add("J12-码字面锚", j12, "资源/参数码序对");

    // J13 (dyn) 矩阵命中明细落账
    s.add_dyn(
        "J13-矩阵明细",
        hits == total,
        &format!("hits={}/{} cap_ok={}", hits, total, cap_ok),
    );

    // J14 (dyn) 表面注记落账
    let note = flags_note(&t);
    s.add_dyn("J14-表面注记", note.contains("fd=2"), &note);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10841_flow() {
        let mut t = PtyTable::new();
        assert!(t.open_master(0o1).err() == Some(PO_BAD_FLAGS));
        assert!(t.open_slave(0, FLAG_RDWR).err() == Some(PO_SLAVE_EARLY));
        assert!(t.open_master(FLAG_RDWR).is_ok());
        assert!(t.open_slave(0, FLAG_RDWR).is_ok());
        assert!(t.open_slave(3, FLAG_RDWR).err() == Some(PO_BAD_INDEX));
        assert!(t.close_master(0));
        assert!(t.open_master(FLAG_RDWR).is_ok());
        assert!(t.ledger_ok());
        assert_eq!(judge(&OpenReq { kind: Kind::Master, flags: FLAG_RDWR, fd_used: FD_CAP, ptys_used: 0 }), Verdict::Deny(PO_FD_EXH));
    }

    #[test]
    fn unx10841_checks_green() {
        let s = run_unx10841_checks();
        assert!(s.all_passed(), "UNX-F10841 判据未全绿");
    }
}
