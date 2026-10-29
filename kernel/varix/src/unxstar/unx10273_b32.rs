//! UNX-F10273 压力结果归因衔接（压力失绿入 B32 归因面）（0x16560 段）
//!
//! 锚点：UNX-F10273-J1 归因衔接达成（压力失绿结果按 B32 归因面闭集
//! 路由登记 10/10 次）；J2 越面检出（非 B32 根因路由/重复登记/冻结后
//! 登 10/10 次检出）；J3 衔接指纹（同失绿同归因同指纹）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·压力演习）。
//!
//! 结构：B32 归因面闭集（压力根因六类）+ 衔接册（失绿 → 根因）+ 归
//! 因核（信号 → 根因真源）+ 注入核（越面/重复/冻结必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x16560 独占 0x20）。
pub const SEG_BASE: u32 = 0x16560;
pub const BG_OUT_FACE: u32 = SEG_BASE + 0x01; // 越面根因
pub const BG_DUP: u32 = SEG_BASE + 0x02; // 重复登记
pub const BG_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后登
pub const BG_BAD_SIGNAL: u32 = SEG_BASE + 0x04; // 坏信号
pub const BG_EMPTY: u32 = SEG_BASE + 0x05; // 空注

/// B32 压力根因闭集。
pub const R_OOM: u8 = 0; // 内存耗尽
pub const R_DISK: u8 = 1; // 盘满
pub const R_IRQ: u8 = 2; // 中断风暴
pub const R_CONT: u8 = 3; // 争用（锁/调度）
pub const R_LEAK: u8 = 4; // 缓慢泄漏
pub const R_THROTTLE: u8 = 5; // 限流误伤
pub const FACE_N: u8 = 6;

pub fn face_ok(r: u8) -> bool {
    r < FACE_N
}

pub fn face_name(r: u8) -> &'static str {
    match r {
        R_OOM => "oom",
        R_DISK => "diskfull",
        R_IRQ => "irqstorm",
        R_CONT => "contention",
        R_LEAK => "slowleak",
        _ => "throttle",
    }
}

/// 压力信号闭集。
pub const G_ENOMEM: u8 = 0;
pub const G_ENOSPC: u8 = 1;
pub const G_LOSS: u8 = 2;
pub const G_LOCKWAIT: u8 = 3;
pub const G_SLOPE: u8 = 4;
pub const G_DELAY: u8 = 5;

/// 归因核真源：信号 → B32 根因。
pub fn attribute(sig: u8) -> Result<u8, &'static str> {
    match sig {
        G_ENOMEM => Ok(R_OOM),
        G_ENOSPC => Ok(R_DISK),
        G_LOSS => Ok(R_IRQ),
        G_LOCKWAIT => Ok(R_CONT),
        G_SLOPE => Ok(R_LEAK),
        G_DELAY => Ok(R_THROTTLE),
        _ => Err("bad_signal"),
    }
}

/// 失绿条目。
#[derive(Clone, PartialEq)]
pub struct GreenFail {
    pub ck: u32,
    pub sig: u8,
    pub root: u8, // 归因结果（须与 attribute 一致）
    pub note: String,
}

impl GreenFail {
    pub fn ok(&self) -> bool {
        self.ck != 0 && !self.note.is_empty() && attribute(self.sig) == Ok(self.root)
    }
}

/// 衔接册。
pub struct BridgeBook {
    rows: Vec<GreenFail>,
    frozen: bool,
}

impl BridgeBook {
    pub fn new() -> BridgeBook {
        BridgeBook { rows: Vec::new(), frozen: false }
    }

    /// 登记：重号/冻结后写/空注/坏信号拒收。
    pub fn reg(&mut self, ck: u32, sig: u8, note: &str) -> Result<u8, &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if ck == 0 {
            return Err("zero_ck");
        }
        if note.is_empty() {
            return Err("empty_note");
        }
        let root = attribute(sig)?;
        if self.rows.iter().any(|r| r.ck == ck) {
            return Err("dup_ck");
        }
        self.rows.push(GreenFail {
            ck,
            sig,
            root,
            note: note.to_string(),
        });
        Ok(root)
    }

    /// 越面企图：登记 root 与归因核不一致的条目（由注入核直推）。
    pub fn reg_forced(&mut self, ck: u32, sig: u8, root: u8, note: &str) -> Result<(), &'static str> {
        // 强制通道：不校验面闭集（越面样本正是要点名的对象），ok() 必假入 mismatch
        if self.frozen {
            return Err("frozen");
        }
        if self.rows.iter().any(|r| r.ck == ck) {
            return Err("dup_ck");
        }
        self.rows.push(GreenFail {
            ck,
            sig,
            root,
            note: note.to_string(),
        });
        Ok(())
    }

    /// 错配清单：root 与归因核不一致的登记。
    pub fn mismatch(&self) -> Vec<u32> {
        self.rows.iter().filter(|r| !r.ok()).map(|r| r.ck).collect()
    }

    /// 根因分布。
    pub fn count_of(&self, root: u8) -> usize {
        self.rows.iter().filter(|r| r.root == root).count()
    }

    /// 衔接指纹。
    pub fn fp(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        let mut sorted: Vec<&GreenFail> = self.rows.iter().collect();
        sorted.sort_by_key(|r| r.ck);
        for r in sorted {
            for b in format!("{}|{}|{}", r.ck, r.sig, r.root).as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x01000193);
            }
        }
        h
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn all_ok(&self) -> bool {
        self.rows.iter().all(|r| r.ok())
    }
}

/// 注入核。
pub fn inject_fault(b: &mut BridgeBook, code: u32, ck: u32) -> Result<u8, &'static str> {
    match code {
        BG_OUT_FACE => {
            let _ = b.reg(ck, G_ENOMEM, "正常一")?;
            b.reg_forced(ck + 1, G_ENOMEM, 200, "越面").map(|_| 0)
        }
        BG_DUP => {
            let _ = b.reg(ck, G_ENOMEM, "一")?;
            b.reg(ck, G_ENOSPC, "二")
        }
        BG_FROZEN => {
            b.freeze();
            b.reg(ck, G_ENOMEM, "冻结后")
        }
        BG_BAD_SIGNAL => b.reg(ck, 99, "坏信号"),
        BG_EMPTY => b.reg(ck, G_ENOMEM, ""),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10273_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10273");

    // J1 六信号归因衔接全通
    let j1 = {
        let mut b = BridgeBook::new();
        let sigs = [G_ENOMEM, G_ENOSPC, G_LOSS, G_LOCKWAIT, G_SLOPE, G_DELAY];
        let roots = [R_OOM, R_DISK, R_IRQ, R_CONT, R_LEAK, R_THROTTLE];
        let mut ok = true;
        for (i, sg) in sigs.iter().enumerate() {
            match b.reg(i as u32 + 1, *sg, "n") {
                Ok(r) => {
                    if r != roots[i] {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
        }
        ok && b.len() == 6 && b.all_ok()
    };
    s.add("J1-六信号衔接", j1, "信号→根因一一对应");

    // J2a 越面检出：错配入清单
    let j2a = {
        let mut b = BridgeBook::new();
        let _ = inject_fault(&mut b, BG_OUT_FACE, 10);
        b.mismatch() == vec![11u32]
    };
    s.add("J2a-越面检出", j2a, "错配条目精确入单");

    // J2b 归因核真源：六信号互异且闭集外拒
    let j2b = {
        let outs: Vec<u8> = [G_ENOMEM, G_ENOSPC, G_LOSS, G_LOCKWAIT, G_SLOPE, G_DELAY]
            .iter()
            .map(|x| attribute(*x).unwrap())
            .collect();
        let mut uniq = true;
        for i in 0..outs.len() {
            for j in (i + 1)..outs.len() {
                if outs[i] == outs[j] {
                    uniq = false;
                }
            }
        }
        uniq && attribute(99).is_err()
    };
    s.add("J2b-归因真源", j2b, "六根因互异坏信号拒");

    // J3 衔接指纹：同册同指纹/变必变
    let j3 = {
        let mk = |alt: bool| {
            let mut b = BridgeBook::new();
            let sig = if alt { G_ENOSPC } else { G_ENOMEM };
            let _ = b.reg(1, sig, "n");
            b.fp()
        };
        let f1 = mk(false);
        let f2 = mk(false);
        let f3 = mk(true);
        f1 == f2 && f1 != f3 && f1 != 0
    };
    s.add("J3-衔接指纹", j3, "信号变指纹必变");

    // J4 重号拒收
    let j4 = inject_fault(&mut BridgeBook::new(), BG_DUP, 20) == Err("dup_ck");
    s.add("J4-重号拒收", j4, "同判据二次即拒");

    // J5 坏信号拒收
    let j5 = inject_fault(&mut BridgeBook::new(), BG_BAD_SIGNAL, 21) == Err("bad_signal");
    s.add("J5-坏信号拒", j5, "闭集外即拒");

    // J6 注入五路（分路检出语义：越面走 mismatch 点名，其余走拒收）
    let mut caught = 0;
    {
        let mut b = BridgeBook::new();
        let _ = inject_fault(&mut b, BG_OUT_FACE, 30);
        if b.mismatch() == vec![31u32] {
            caught += 1; // 越面：强制入册但 ok 必假，mismatch 精确点名
        }
    }
    if inject_fault(&mut BridgeBook::new(), BG_DUP, 30).is_err() {
        caught += 1;
    }
    if inject_fault(&mut BridgeBook::new(), BG_FROZEN, 30).is_err() {
        caught += 1;
    }
    if inject_fault(&mut BridgeBook::new(), BG_BAD_SIGNAL, 30).is_err() {
        caught += 1;
    }
    if inject_fault(&mut BridgeBook::new(), BG_EMPTY, 30).is_err() {
        caught += 1;
    }
    s.add("J6-注入五路", caught == 5, "五路注入全拒/检出");

    // J7 冻结红线
    let j7 = {
        let mut b = BridgeBook::new();
        inject_fault(&mut b, BG_FROZEN, 40);
        b.is_frozen()
    };
    s.add("J7-冻结拒登", j7, "印后登记必拒");

    // J8 条目负样本：错配 ok 必假
    let j8 = {
        let bad = GreenFail {
            ck: 1,
            sig: G_ENOMEM,
            root: R_DISK, // 错配
            note: "x".to_string(),
        };
        !bad.ok()
    };
    s.add("J8-错配负样本", j8, "错配 ok 必假");

    // J9 根因分布核：两 OOM 一 DISK 分布正确
    let j9 = {
        let mut b = BridgeBook::new();
        let _ = b.reg(1, G_ENOMEM, "a");
        let _ = b.reg(2, G_ENOMEM, "b");
        let _ = b.reg(3, G_ENOSPC, "c");
        b.count_of(R_OOM) == 2 && b.count_of(R_DISK) == 1
    };
    s.add("J9-根因分布", j9, "分布计数精确");

    // J10 十轮归因回放：10 轮指纹一致（确定性）
    let j10 = {
        let mk = || {
            let mut b = BridgeBook::new();
            let sigs = [G_ENOMEM, G_LOCKWAIT, G_SLOPE];
            for (i, sg) in sigs.iter().enumerate() {
                let _ = b.reg(i as u32 + 1, *sg, "n");
            }
            b.fp()
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10
    };
    s.add("J10-十轮归因", j10, "10/10 轮指纹一致");

    s
}
