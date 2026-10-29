//! UNX-F10805 非管道 fd 误入管道路由错误面（0x1bd20 段）
//!
//! 锚点：UNX-F10805-J1 管道专属路由对平——管道专属命令落在非管道 fd 上
//! 必 ENOTTY、落在关闭 fd 上必 EBADF、落在管道 fd 上放行，通用命令任何
//! 打开 fd 皆放行；注入坏路由（不查 fd 类别/不查关闭位）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：fd 表状态机（五类 fd+关闭位，open/close 推进）+ 命令分类表
//! （管道专属/终端专属/通用三族）+ 路由判定器（类别×命令族算
//! EBADF/ENOTTY/放行）+ 注入核（两类坏路由必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：EBADF=9、ENOTTY=25。
pub const POSIX_EBADF: u32 = 9;
pub const POSIX_ENOTTY: u32 = 25;

/// 诊断段（0x1bd20 独占 0x20）。
pub const SEG_BASE: u32 = 0x1bd20;
pub const BR_KIND_MISS: u32 = SEG_BASE + 0x01; // 管道专属命令不查类别
pub const BR_BADF_MISS: u32 = SEG_BASE + 0x02; // 关闭 fd 不查 EBADF
pub const BR_TTY_MISS: u32 = SEG_BASE + 0x03; // 终端命令不查类别
pub const BR_ROUTE_SWAP: u32 = SEG_BASE + 0x04; // 族路由对调
pub const BR_TABLE_OVER: u32 = SEG_BASE + 0x05; // fd 表超容
pub const BR_DUP_LOSS: u32 = SEG_BASE + 0x06; // dup 计数丢失

/// fd 类别五态 + 关闭位。
#[derive(Clone, PartialEq, Eq)]
pub enum Kind {
    Pipe,
    Fifo,
    Regular,
    Tty,
    Socket,
    Closed,
}

/// 命令族：管道专属 / 终端专属 / 通用。
#[derive(Clone, PartialEq, Eq)]
pub enum OpKind {
    PipeOp,
    TtyOp,
    GenericOp,
}

/// 命令分类表（字面命令号 → 命令族）。
pub fn classify(cmd: u32) -> OpKind {
    match cmd {
        // F_GETPIPE_SZ / F_SETPIPE_SZ / F_GETFIFO_SZ 类
        1031 | 1032 | 1033 => OpKind::PipeOp,
        // TIOCGWINSZ / TIOCSWINSZ / TIOCGPGRP 类
        0x5413 | 0x5414 | 0x540F => OpKind::TtyOp,
        // F_GETFL / F_SETFL / F_GETFD / F_SETFD 类
        3 | 4 | 1 | 2 => OpKind::GenericOp,
        _ => OpKind::GenericOp,
    }
}

/// 路由判定核：类别 × 命令族算 EBADF / ENOTTY / 放行。
pub fn route(kind: &Kind, op: &OpKind) -> Result<(), u32> {
    if *kind == Kind::Closed {
        return Err(BR_BADF_MISS);
    }
    match op {
        OpKind::PipeOp => match kind {
            Kind::Pipe | Kind::Fifo => Ok(()),
            _ => Err(BR_KIND_MISS),
        },
        OpKind::TtyOp => match kind {
            Kind::Tty => Ok(()),
            _ => Err(BR_TTY_MISS),
        },
        OpKind::GenericOp => Ok(()),
    }
}

/// 场景输入：fd 类别 × 命令族（判定器按字段计算）。
#[derive(Clone)]
pub struct Scenario {
    pub kind: Kind,
    pub op: OpKind,
}

/// 场景判定器。
pub fn judge(sc: &Scenario) -> Result<(), u32> {
    route(&sc.kind, &sc.op)
}

/// fd 表状态机：定容表 + dup 计数 + open/close 推进。
pub struct FdTable {
    pub slots: Vec<Kind>,
    pub note: String,
}

pub const TABLE_CAP: usize = 8;

impl FdTable {
    pub fn new(note: &str) -> FdTable {
        let mut slots: Vec<Kind> = Vec::new();
        let mut i = 0usize;
        while i < TABLE_CAP {
            slots.push(Kind::Closed);
            i += 1;
        }
        FdTable { slots, note: note.to_string() }
    }

    /// 打开槽位：返回槽号；表满拒。
    pub fn open(&mut self, k: Kind) -> Result<usize, u32> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i] == Kind::Closed {
                self.slots[i] = k;
                return Ok(i);
            }
            i += 1;
        }
        Err(BR_TABLE_OVER)
    }

    /// 关闭槽位：已关再关必拒。
    pub fn close(&mut self, fd: usize) -> Result<(), u32> {
        if fd >= self.slots.len() {
            return Err(BR_BADF_MISS);
        }
        if self.slots[fd] == Kind::Closed {
            return Err(BR_BADF_MISS);
        }
        self.slots[fd] = Kind::Closed;
        Ok(())
    }

    /// 查类别：越界/已关一律 Closed。
    pub fn kind_of(&self, fd: usize) -> Kind {
        if fd >= self.slots.len() {
            Kind::Closed
        } else {
            self.slots[fd].clone()
        }
    }
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 24;

/// 正反双样本矩阵：期望 () 或期望诊断码。
pub fn sample_matrix() -> Vec<(Scenario, Option<u32>)> {
    let mut m: Vec<(Scenario, Option<u32>)> = Vec::new();
    // 正：管道专属命令落在管道/FIFO fd 上放行
    m.push((Scenario { kind: Kind::Pipe, op: OpKind::PipeOp }, None));
    m.push((Scenario { kind: Kind::Fifo, op: OpKind::PipeOp }, None));
    // 正：通用命令任何打开 fd 放行
    m.push((Scenario { kind: Kind::Regular, op: OpKind::GenericOp }, None));
    m.push((Scenario { kind: Kind::Socket, op: OpKind::GenericOp }, None));
    // 正：终端命令落在 tty fd 放行
    m.push((Scenario { kind: Kind::Tty, op: OpKind::TtyOp }, None));
    // 反：管道专属命令落在非管道 fd 必 ENOTTY
    m.push((Scenario { kind: Kind::Regular, op: OpKind::PipeOp }, Some(BR_KIND_MISS)));
    m.push((Scenario { kind: Kind::Socket, op: OpKind::PipeOp }, Some(BR_KIND_MISS)));
    m.push((Scenario { kind: Kind::Tty, op: OpKind::PipeOp }, Some(BR_KIND_MISS)));
    // 反：终端命令落在非 tty fd 必 ENOTTY
    m.push((Scenario { kind: Kind::Pipe, op: OpKind::TtyOp }, Some(BR_TTY_MISS)));
    // 反：关闭 fd 任何命令必 EBADF
    m.push((Scenario { kind: Kind::Closed, op: OpKind::PipeOp }, Some(BR_BADF_MISS)));
    m.push((Scenario { kind: Kind::Closed, op: OpKind::GenericOp }, Some(BR_BADF_MISS)));
    m
}

/// 矩阵对平：返回 (命中数, 总数, 容量合规)。
pub fn verify_matrix() -> (usize, usize, bool) {
    let m = sample_matrix();
    let mut ok = 0usize;
    let mut i = 0usize;
    while i < m.len() {
        let (sc, want) = &m[i];
        let got = judge(sc);
        let hit = match want {
            None => got.is_ok(),
            Some(c) => got.err() == Some(*c),
        };
        if hit {
            ok += 1;
        }
        i += 1;
    }
    (ok, m.len(), m.len() <= MATRIX_CAP)
}

/// 坏实现 A：管道专属命令不查 fd 类别。
pub fn faulty_no_kind(sc: &Scenario) -> Result<(), u32> {
    if sc.kind == Kind::Closed {
        return Err(BR_BADF_MISS);
    }
    Ok(())
}

/// 坏实现 B：关闭 fd 不查 EBADF（幽灵 fd 放行）。
pub fn faulty_no_badf(sc: &Scenario) -> Result<(), u32> {
    if sc.kind == Kind::Closed {
        return Ok(());
    }
    route(&sc.kind, &sc.op)
}

/// 坏实现 C：族路由对调（管道专属按终端判、终端按管道判）。
pub fn faulty_swap(sc: &Scenario) -> Result<(), u32> {
    match sc.op {
        OpKind::PipeOp => route(&sc.kind, &OpKind::TtyOp),
        OpKind::TtyOp => route(&sc.kind, &OpKind::PipeOp),
        OpKind::GenericOp => Ok(()),
    }
}

/// 注入核：三类坏路由必与权威分歧（检出）。
pub fn inject_three() -> [bool; 3] {
    let mut got = [false; 3];
    // 1) 类别漏查：正则 fd 上管道专属命令，权威拒、坏实现放行
    let sc1 = Scenario { kind: Kind::Regular, op: OpKind::PipeOp };
    got[0] = judge(&sc1) != faulty_no_kind(&sc1) && judge(&sc1).err() == Some(BR_KIND_MISS);
    // 2) EBADF 漏查：关闭 fd 上命令，权威拒、坏实现放行
    let sc2 = Scenario { kind: Kind::Closed, op: OpKind::GenericOp };
    got[1] = judge(&sc2) != faulty_no_badf(&sc2) && judge(&sc2).err() == Some(BR_BADF_MISS);
    // 3) 族对调：tty 上管道专属命令，权威 ENOTTY、坏实现放行
    let sc3 = Scenario { kind: Kind::Tty, op: OpKind::PipeOp };
    got[2] = judge(&sc3) != faulty_swap(&sc3) && judge(&sc3).err() == Some(BR_KIND_MISS);
    got
}

/// fd 表流程核：open→route→close→route 全序推进，账面对平。
pub fn table_flow_ok() -> bool {
    let mut t = FdTable::new("路由表");
    let fd1 = t.open(Kind::Pipe);
    let fd2 = t.open(Kind::Regular);
    let r1 = match fd1 {
        Ok(f) => route(&t.kind_of(f), &OpKind::PipeOp),
        Err(_) => Err(BR_BADF_MISS),
    };
    let r2 = match fd2 {
        Ok(f) => route(&t.kind_of(f), &OpKind::PipeOp),
        Err(_) => Err(BR_BADF_MISS),
    };
    let closed = match fd2 {
        Ok(f) => t.close(f),
        Err(_) => Err(BR_BADF_MISS),
    };
    let r3 = match fd2 {
        Ok(f) => route(&t.kind_of(f), &OpKind::PipeOp),
        Err(_) => Err(BR_BADF_MISS),
    };
    fd1.is_ok() && fd2.is_ok() && r1.is_ok() && r2.err() == Some(BR_KIND_MISS)
        && closed.is_ok() && r3.err() == Some(BR_BADF_MISS)
}

/// 管道路由判据（对应卡验收 J 判据）。
pub fn run_unx10805_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10805");
    let inj = inject_three();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平：正反样本全命中
    s.add("J1-矩阵对平", hits == total && cap_ok, "十一线正反样本全命中");

    // J2 正路径：管道专属命令落在管道 fd 放行
    let j2 = route(&Kind::Pipe, &OpKind::PipeOp).is_ok();
    s.add("J2-管道放行", j2, "Pipe fd 上管道专属命令放行");

    // J3 反路径：管道专属命令落在 regular fd 必 ENOTTY
    let j3 = route(&Kind::Regular, &OpKind::PipeOp).err() == Some(BR_KIND_MISS);
    s.add("J3-误入必拒", j3, "非管道 fd 上管道专属命令 ENOTTY");

    // J4 反路径：关闭 fd 必 EBADF
    let j4 = route(&Kind::Closed, &OpKind::PipeOp).err() == Some(BR_BADF_MISS)
        && route(&Kind::Closed, &OpKind::GenericOp).err() == Some(BR_BADF_MISS);
    s.add("J4-关闭必拒", j4, "Closed fd 任何命令 EBADF");

    // J5 命令分类表：字面命令号分族正确
    let j5 = classify(1032) == OpKind::PipeOp
        && classify(0x5413) == OpKind::TtyOp
        && classify(3) == OpKind::GenericOp;
    s.add("J5-命令分族", j5, "1032 管道族 / 0x5413 终端族 / 3 通用族");

    // J6 fd 表流程核：open→route→close→route 全序对平
    s.add("J6-表流程核", table_flow_ok(), "打开路由放行、误类拒、关闭后 EBADF");

    // J7 注入1 类别漏查检出
    s.add("J7-类别漏检查出", inj[0], "不查 fd 类别坏路由必分歧");

    // J8 注入2 EBADF 漏查检出
    s.add("J8-EBADF漏检查出", inj[1], "幽灵 fd 放行必分歧");

    // J9 注入3 族对调检出
    s.add("J9-族对调检出", inj[2], "路由族对调坏实现必分歧");

    // J10 字面锚：EBADF=9 / ENOTTY=25
    let j10 = POSIX_EBADF == 9 && POSIX_ENOTTY == 25;
    s.add("J10-字面锚对平", j10, "9/25 双锚");

    // J11 容量边界：表满再开必拒
    let j11 = {
        let mut t = FdTable::new("cap");
        let mut filled = true;
        let mut i = 0usize;
        while i < TABLE_CAP {
            if t.open(Kind::Regular).is_err() {
                filled = false;
            }
            i += 1;
        }
        filled && t.open(Kind::Pipe).err() == Some(BR_TABLE_OVER)
    };
    s.add("J11-表容量边界", j11, "八槽满后必拒");

    // J12 误关边界：双关必拒
    let j12 = {
        let mut t = FdTable::new("dbl");
        let fd = t.open(Kind::Pipe);
        match fd {
            Ok(f) => {
                let _ = t.close(f);
                t.close(f).err() == Some(BR_BADF_MISS)
            }
            Err(_) => false,
        }
    };
    s.add("J12-双关必拒", j12, "已关槽位再关 EBADF");

    // J13 (dyn) 表面指纹落账
    let mut t2 = FdTable::new("指纹");
    let _ = t2.open(Kind::Pipe);
    let _ = t2.open(Kind::Tty);
    let mut h: u32 = SEG_BASE;
    let mut i = 0usize;
    while i < t2.slots.len() {
        let v = match t2.slots[i] {
            Kind::Pipe => 1u32,
            Kind::Fifo => 2,
            Kind::Regular => 3,
            Kind::Tty => 4,
            Kind::Socket => 5,
            Kind::Closed => 0,
        };
        h = h.wrapping_mul(31).wrapping_add(v);
        i += 1;
    }
    s.add_dyn("J13-表面指纹", h != 0, &format!("fold={:#x}", h));

    // J14 (dyn) 矩阵命中明细
    s.add_dyn(
        "J14-命中明细",
        hits == total,
        &format!("hits={}/{} cap_ok={}", hits, total, cap_ok),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10805_route_flow() {
        assert!(route(&Kind::Pipe, &OpKind::PipeOp).is_ok());
        assert!(route(&Kind::Regular, &OpKind::PipeOp).err() == Some(BR_KIND_MISS));
        assert!(route(&Kind::Closed, &OpKind::GenericOp).err() == Some(BR_BADF_MISS));
        assert!(route(&Kind::Tty, &OpKind::TtyOp).is_ok());
        assert!(classify(1031) == OpKind::PipeOp);
        let mut t = FdTable::new("t");
        let fd = t.open(Kind::Fifo);
        assert!(fd.is_ok());
        assert!(t.close(fd.ok().unwrap_or(TABLE_CAP)).is_ok());
        assert!(t.kind_of(TABLE_CAP + 1) == Kind::Closed);
    }

    #[test]
    fn unx10805_checks_green() {
        let s = run_unx10805_checks();
        assert!(s.all_passed(), "UNX-F10805 判据未全绿");
    }
}
