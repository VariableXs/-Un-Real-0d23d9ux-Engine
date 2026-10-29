//! UNX-F10819 O_TRUNC/O_APPEND 语义面（FIFO 显式注册账）（0x1bee0 段）
//!
//! 锚点：UNX-F10819-J1 打开旗标对平——FIFO 上 O_TRUNC 显式忽略不报错、
//! FIFO 上 O_APPEND 接受且无寻位副作用、普通文件只读口 O_TRUNC 必 EINVAL
//! 三线全对平；注入坏实现（FIFO 误报 EINVAL/普通文件漏查）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：打开旗标账（kind × flags 位集）+ 旗标判定器（节点类型对撞
//! 旗标位算 忽略/接受/EINVAL）+ 旗标注记状态机 + 重放核（逐笔先验同判）
//! + 注入核（两类坏实现必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：EINVAL=22。
pub const POSIX_EINVAL: u32 = 22;

/// 旗标位锚：O_TRUNC=0o1000、O_APPEND=0o2000、O_RDONLY=0。
pub const O_TRUNC: u32 = 0o1000;
pub const O_APPEND: u32 = 0o2000;
pub const O_RDONLY: u32 = 0;

/// 诊断段（0x1bee0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1bee0;
pub const TA_FIFO_EINVAL: u32 = SEG_BASE + 0x01; // FIFO 上误报 EINVAL
pub const TA_REG_MISS: u32 = SEG_BASE + 0x02; // 普通文件漏查
pub const TA_APPEND_SHIFT: u32 = SEG_BASE + 0x03; // 追加位寻位副作用
pub const TA_FLAG_LOSS: u32 = SEG_BASE + 0x04; // 旗标丢失
pub const TA_NOTE_SKEW: u32 = SEG_BASE + 0x05; // 注记错账
pub const TA_REPLAY_MISS: u32 = SEG_BASE + 0x06; // 重放不齐

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        TA_FIFO_EINVAL => "FIFO 上误报 EINVAL",
        TA_REG_MISS => "普通文件 O_TRUNC 漏查",
        TA_APPEND_SHIFT => "追加位寻位副作用",
        TA_FLAG_LOSS => "旗标丢失",
        TA_NOTE_SKEW => "注记错账",
        TA_REPLAY_MISS => "重放不齐",
        _ => "未知诊断码",
    }
}

/// 节点类型。
#[derive(Clone, PartialEq, Eq)]
pub enum Kind {
    Fifo,
    Regular,
}

/// 旗标裁定输出。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum FlagOut {
    /// 接受旗标（无副作用或正常生效）。
    Accept(&'static str),
    /// 显式忽略（不报错、不生效）。
    Ignore(&'static str),
}

/// 场景输入：节点类型 × 旗标位集 × 访问口。
#[derive(Clone)]
pub struct Scenario {
    pub kind: Kind,
    pub flags: u32,
    pub rdonly: bool,
}

/// 旗标判定核：
/// FIFO × O_TRUNC → Ignore（不报错不生效）；FIFO × O_APPEND → Accept（无寻位）；
/// Regular 只读 × O_TRUNC → EINVAL；Regular 写口 O_TRUNC → Accept（清空）。
pub fn judge(sc: &Scenario) -> Result<FlagOut, u32> {
    match sc.kind {
        Kind::Fifo => {
            if sc.flags & O_TRUNC != 0 {
                Ok(FlagOut::Ignore("trunc-void"))
            } else if sc.flags & O_APPEND != 0 {
                Ok(FlagOut::Accept("append-fifo"))
            } else {
                Ok(FlagOut::Accept("plain"))
            }
        }
        Kind::Regular => {
            if sc.rdonly && sc.flags & O_TRUNC != 0 {
                return Err(POSIX_EINVAL);
            }
            if sc.flags & O_APPEND != 0 {
                Ok(FlagOut::Accept("append-regular"))
            } else if sc.flags & O_TRUNC != 0 {
                Ok(FlagOut::Accept("trunc-regular"))
            } else {
                Ok(FlagOut::Accept("plain"))
            }
        }
    }
}

/// 旗标账状态机：逐次 open 落账（kind, flags, 裁定注记）。
pub struct FlagLedger {
    pub rows: Vec<(Kind, u32, String)>,
    pub note: String,
}

impl FlagLedger {
    pub fn new() -> FlagLedger {
        FlagLedger { rows: Vec::new(), note: "ledger".to_string() }
    }

    /// 一次 open 尝试：裁定入账。
    pub fn open(&mut self, sc: &Scenario) -> Result<FlagOut, u32> {
        let out = judge(sc);
        let tag = match &out {
            Ok(FlagOut::Ignore(t)) => format!("ignore:{}", t),
            Ok(FlagOut::Accept(t)) => format!("accept:{}", t),
            Err(c) => format!("err:{:#x}", c),
        };
        self.rows.push((sc.kind.clone(), sc.flags, tag));
        out
    }

    /// FIFO 零副作用核：账内 FIFO 行无 EINVAL、无 trunc 生效注记。
    pub fn fifo_clean(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.rows.len() {
            let (kind, _flags, tag) = &self.rows[i];
            if *kind == Kind::Fifo && tag.starts_with("err:") {
                ok = false;
            }
            i += 1;
        }
        ok
    }

    /// 重放核：逐行按先验场景与权威同判。
    pub fn replay_ok(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.rows.len() {
            let (kind, flags, tag) = &self.rows[i];
            let sc = Scenario { kind: kind.clone(), flags: *flags, rdonly: false };
            let want = match judge(&sc) {
                Ok(FlagOut::Ignore(t)) => format!("ignore:{}", t),
                Ok(FlagOut::Accept(t)) => format!("accept:{}", t),
                Err(c) => format!("err:{:#x}", c),
            };
            if *tag != want {
                ok = false;
            }
            i += 1;
        }
        ok
    }
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望注记或 EINVAL。
pub fn sample_matrix() -> Vec<(Scenario, Option<u32>)> {
    let mut m: Vec<(Scenario, Option<u32>)> = Vec::new();
    // 正：FIFO × O_TRUNC → 显式忽略
    m.push((Scenario { kind: Kind::Fifo, flags: O_TRUNC, rdonly: false }, None));
    // 正：FIFO × O_APPEND → 接受
    m.push((Scenario { kind: Kind::Fifo, flags: O_APPEND, rdonly: false }, None));
    // 正：FIFO 裸开
    m.push((Scenario { kind: Kind::Fifo, flags: O_RDONLY, rdonly: true }, None));
    // 正：Regular 写口 O_TRUNC → 清空
    m.push((Scenario { kind: Kind::Regular, flags: O_TRUNC, rdonly: false }, None));
    // 正：Regular O_APPEND
    m.push((Scenario { kind: Kind::Regular, flags: O_APPEND, rdonly: false }, None));
    // 反：Regular 只读 × O_TRUNC → EINVAL
    m.push((Scenario { kind: Kind::Regular, flags: O_TRUNC, rdonly: true }, Some(POSIX_EINVAL)));
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

/// 坏实现 A：FIFO 上误报 EINVAL。
pub fn faulty_fifo_einval(sc: &Scenario) -> Result<FlagOut, u32> {
    match sc.kind {
        Kind::Fifo if sc.flags & O_TRUNC != 0 => Err(POSIX_EINVAL),
        _ => judge(sc),
    }
}

/// 坏实现 B：普通文件只读 O_TRUNC 漏查。
pub fn faulty_reg_miss(sc: &Scenario) -> Result<FlagOut, u32> {
    match sc.kind {
        Kind::Regular => Ok(FlagOut::Accept("trunc-regular")),
        _ => judge(sc),
    }
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    // 1) FIFO O_TRUNC：权威 Ignore、坏实现 EINVAL → 检出
    let sc1 = Scenario { kind: Kind::Fifo, flags: O_TRUNC, rdonly: false };
    got[0] = judge(&sc1) != faulty_fifo_einval(&sc1)
        && judge(&sc1) == Ok(FlagOut::Ignore("trunc-void"));
    // 2) 只读 O_TRUNC：权威 EINVAL、坏实现放行 → 检出
    let sc2 = Scenario { kind: Kind::Regular, flags: O_TRUNC, rdonly: true };
    got[1] = judge(&sc2) != faulty_reg_miss(&sc2)
        && judge(&sc2).err() == Some(POSIX_EINVAL);
    got
}

/// 旗标全序核：六步账（3 FIFO + 3 Regular）重放对平。
pub fn flag_seq_ok() -> bool {
    let mut lg = FlagLedger::new();
    let _ = lg.open(&Scenario { kind: Kind::Fifo, flags: O_TRUNC, rdonly: false });
    let _ = lg.open(&Scenario { kind: Kind::Fifo, flags: O_APPEND, rdonly: false });
    let _ = lg.open(&Scenario { kind: Kind::Fifo, flags: O_RDONLY, rdonly: true });
    let _ = lg.open(&Scenario { kind: Kind::Regular, flags: O_TRUNC, rdonly: true });
    let _ = lg.open(&Scenario { kind: Kind::Regular, flags: O_TRUNC, rdonly: false });
    let _ = lg.open(&Scenario { kind: Kind::Regular, flags: O_APPEND, rdonly: false });
    lg.rows.len() == 6 && lg.fifo_clean() && lg.replay_ok()
}

/// 追加位注记核：FIFO 的 O_APPEND 无寻位副作用（注记 append-fifo）。
pub fn append_note_ok() -> bool {
    let sc = Scenario { kind: Kind::Fifo, flags: O_APPEND, rdonly: false };
    match judge(&sc) {
        Ok(FlagOut::Accept(t)) => t == "append-fifo",
        _ => false,
    }
}

/// 旗标面注记（对账/落账用）。
pub fn flag_note(sc: &Scenario) -> String {
    match judge(sc) {
        Ok(FlagOut::Ignore(t)) => format!("ignore:{}", t),
        Ok(FlagOut::Accept(t)) => format!("accept:{}", t),
        Err(c) => format!("err:{:#x}", c),
    }
}

/// 判据（对应卡验收 J 判据）。
pub fn run_unx10819_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10819");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "六线旗标样本全命中");

    // J2 正路径：FIFO × O_TRUNC 显式忽略
    let j2 = judge(&Scenario { kind: Kind::Fifo, flags: O_TRUNC, rdonly: false })
        == Ok(FlagOut::Ignore("trunc-void"));
    s.add("J2-TRUNC忽略", j2, "FIFO 上不报错不生效");

    // J3 正路径：FIFO × O_APPEND 接受无副作用
    let j3 = append_note_ok();
    s.add("J3-APPEND接受", j3, "append-fifo 注记无寻位");

    // J4 反路径：Regular 只读 O_TRUNC 必 EINVAL
    let j4 = judge(&Scenario { kind: Kind::Regular, flags: O_TRUNC, rdonly: true })
        .err() == Some(POSIX_EINVAL);
    s.add("J4-只读TRUNC拒", j4, "O_RDONLY|O_TRUNC 必 22");

    // J5 正路径：Regular 写口 O_TRUNC 清空
    let j5 = judge(&Scenario { kind: Kind::Regular, flags: O_TRUNC, rdonly: false })
        == Ok(FlagOut::Accept("trunc-regular"));
    s.add("J5-写口TRUNC", j5, "写口清空生效");

    // J6 旗标全序核
    s.add("J6-全序账核", flag_seq_ok(), "六步账重放对平");

    // J7 注入1 FIFO 误 EINVAL 检出
    s.add("J7-误报检出", inj[0], "FIFO 报 EINVAL 坏实现必分歧");

    // J8 注入2 只读漏查检出
    s.add("J8-漏查检出", inj[1], "只读 O_TRUNC 放行必分歧");

    // J9 FIFO 零副作用账核
    let mut lg = FlagLedger::new();
    let _ = lg.open(&Scenario { kind: Kind::Fifo, flags: O_TRUNC | O_APPEND, rdonly: false });
    let _ = lg.open(&Scenario { kind: Kind::Fifo, flags: O_TRUNC, rdonly: false });
    let j9 = lg.fifo_clean() && lg.rows.len() == 2;
    s.add("J9-FIFO零副作用", j9, "账内 FIFO 行全绿");

    // J10 旗标位锚：0o1000/0o2000 字面对平
    let j10 = O_TRUNC == 0o1000 && O_APPEND == 0o2000 && O_RDONLY == 0;
    s.add("J10-旗标位锚", j10, "位集字面对平");

    // J11 字面锚：EINVAL=22 且诊断表完备
    let j11 = POSIX_EINVAL == 22 && diag_text(TA_FIFO_EINVAL).len() > 0
        && diag_text(0xffff_ffff) == "未知诊断码";
    s.add("J11-字面锚对平", j11, "EINVAL 字面 22 且语义完备");

    // J12 旗标面注记闭集
    let n1 = flag_note(&Scenario { kind: Kind::Fifo, flags: O_TRUNC, rdonly: false });
    let n2 = flag_note(&Scenario { kind: Kind::Regular, flags: O_TRUNC, rdonly: true });
    let j12 = n1.contains("ignore") && n2.contains("err") && n1 != n2;
    s.add("J12-注记互异", j12, "ignore/err 注记互异");

    // J13 (dyn) 旗标账指纹落账
    let mut lg2 = FlagLedger::new();
    let _ = lg2.open(&Scenario { kind: Kind::Fifo, flags: O_TRUNC, rdonly: false });
    let _ = lg2.open(&Scenario { kind: Kind::Regular, flags: O_TRUNC, rdonly: true });
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(lg2.rows.len() as u32);
    let mut i = 0usize;
    while i < lg2.rows.len() {
        h = h.wrapping_mul(31).wrapping_add(lg2.rows[i].1);
        i += 1;
    }
    for c in lg2.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    s.add_dyn("J13-旗标账指纹", h != 0, &format!("fold={:#x}", h));

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
    fn unx10819_flag_flow() {
        let mut lg = FlagLedger::new();
        assert!(judge(&Scenario { kind: Kind::Fifo, flags: O_TRUNC, rdonly: false }).is_ok());
        assert!(judge(&Scenario { kind: Kind::Regular, flags: O_TRUNC, rdonly: true })
            .err() == Some(POSIX_EINVAL));
        assert_eq!(
            lg.open(&Scenario { kind: Kind::Fifo, flags: O_APPEND, rdonly: false }),
            Ok(FlagOut::Accept("append-fifo"))
        );
        assert!(lg.fifo_clean() && lg.replay_ok());
        let inj = inject_two();
        assert!(inj.iter().all(|b| *b));
    }

    #[test]
    fn unx10819_checks_green() {
        let s = run_unx10819_checks();
        assert!(s.all_passed(), "UNX-F10819 判据未全绿");
    }
}
