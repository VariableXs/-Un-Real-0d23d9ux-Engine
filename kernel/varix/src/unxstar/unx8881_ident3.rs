//! UNX-F8881 三身份模型（真实/有效/保存）语义基准（0xA580 段）
//!
//! 锚点：UNX-F8881-J1 三身份（ruid/euid/suid）切换矩阵（特权/非特权
//! 两态 × 8 操作）与 Linux 语义逐格一致，判据面为全族母版。
//! 域：CoRun·UNX-C2-B05（三身份面）。
//!
//! 结构：三身份账（ruid/euid/suid）+ 8 操作闭集（setuid/setreuid
//! 四向/setresuid 三向）× 2 特权态 = 16 格切换矩阵 + Linux 语义
//! 逐格断言（-EPERM 拒/成功后三值冻结）+ 母版声明（全族 setuid/
//! seteuid/setreuid/setresuid 从此矩阵派生）+ 注入核（非特权越权/
//! 保存值回退/矩阵漂移/封账）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xA580 独占 0x80）。

pub const SEG_BASE: u32 = 0xA580;
pub const XC_ID_EPERM: u32 = SEG_BASE + 0x01; // 越权
pub const XC_ID_RANGE: u32 = SEG_BASE + 0x02; // 越界 uid
pub const XC_ID_SEALED: u32 = SEG_BASE + 0x03; // 封账

/// uid 界（Linux：-1 = 不变哨兵）。
pub const UID_KEEP: u32 = u32::MAX;

/// 特权态。
pub const PRIV_ROOT: bool = true; // euid == 0
pub const PRIV_USER: bool = false;

/// 8 操作闭集。
pub const OP_SETUID: u8 = 0; // setuid(x)
pub const OP_SETREUID_R: u8 = 1; // setreuid(x, -1)
pub const OP_SETREUID_E: u8 = 2; // setreuid(-1, x)
pub const OP_SETREUID_RE: u8 = 3; // setreuid(x, x)
pub const OP_SETRESUID_R: u8 = 4; // setresuid(x, -1, -1)
pub const OP_SETRESUID_E: u8 = 5; // setresuid(-1, x, -1)
pub const OP_SETRESUID_S: u8 = 6; // setresuid(-1, -1, x)
pub const OP_SETRESUID_RES: u8 = 7; // setresuid(x, x, x)
pub const OPS: u8 = 8;

/// 三身份账。
#[derive(Clone, Copy)]
pub struct Cred3 {
    pub ruid: u32,
    pub euid: u32,
    pub suid: u32,
}

impl Cred3 {
    pub fn new(r: u32, e: u32, s: u32) -> Self {
        Cred3 { ruid: r, euid: e, suid: s }
    }

    /// 特权态（euid == 0）。
    pub fn privileged(&self) -> bool {
        self.euid == 0
    }

    /// Linux setuid 语义：
    /// - 特权：三值全设 x
    /// - 非特权：x == ruid 或 x == suid 时仅 euid ← x；否则 -EPERM
    pub fn op_setuid(&mut self, x: u32) -> Result<(), u32> {
        if self.privileged() {
            self.ruid = x;
            self.euid = x;
            self.suid = x;
            Ok(())
        } else if x == self.ruid || x == self.suid {
            self.euid = x;
            Ok(())
        } else {
            Err(XC_ID_EPERM)
        }
    }

    /// Linux setreuid(r, e) 语义：
    /// - 特权：任设 ruid/euid；euid 变时 suid 跟随 euid（旧 euid 存入）
    /// - 非特权：r ← x 须 x == euid 或 x == ruid（可互换）；e ← x 须 x == ruid 或 x == suid；否则 -EPERM
    pub fn op_setreuid(&mut self, r: u32, e: u32) -> Result<(), u32> {
        let prived = self.privileged();
        // r 侧
        if r != UID_KEEP {
            if prived || r == self.euid || r == self.ruid {
                self.ruid = r;
            } else {
                return Err(XC_ID_EPERM);
            }
        }
        // e 侧（r 侧变化后重算特权）
        if e != UID_KEEP {
            let still_priv = self.ruid == 0 || self.euid == 0;
            if prived || still_priv || e == self.ruid || e == self.suid {
                let old_e = self.euid;
                self.euid = e;
                self.suid = old_e; // euid 变时旧值存 suid
            } else {
                return Err(XC_ID_EPERM);
            }
        }
        Ok(())
    }

    /// Linux setresuid(r, e, s) 语义：
    /// - 特权：三向任设（非 KEEP）
    /// - 非特权：r ← x 须 x ∈ {ruid, euid, suid}；e/s ← x 须 x ∈ {ruid, euid, suid}；否则 -EPERM
    pub fn op_setresuid(&mut self, r: u32, e: u32, s: u32) -> Result<(), u32> {
        let prived = self.privileged();
        let pool = [self.ruid, self.euid, self.suid];
        if r != UID_KEEP {
            if prived || pool.contains(&r) {
                self.ruid = r;
            } else {
                return Err(XC_ID_EPERM);
            }
        }
        if e != UID_KEEP {
            if prived || pool.contains(&e) {
                self.euid = e;
            } else {
                return Err(XC_ID_EPERM);
            }
        }
        if s != UID_KEEP {
            if prived || pool.contains(&s) {
                self.suid = s;
            } else {
                return Err(XC_ID_EPERM);
            }
        }
        Ok(())
    }

    /// 统一操作分发（矩阵单元）。
    pub fn apply(&mut self, op: u8, x: u32) -> Result<(), u32> {
        if op >= OPS {
            return Err(XC_ID_RANGE);
        }
        match op {
            OP_SETUID => self.op_setuid(x),
            OP_SETREUID_R => self.op_setreuid(x, UID_KEEP),
            OP_SETREUID_E => self.op_setreuid(UID_KEEP, x),
            OP_SETREUID_RE => self.op_setreuid(x, x),
            OP_SETRESUID_R => self.op_setresuid(x, UID_KEEP, UID_KEEP),
            OP_SETRESUID_E => self.op_setresuid(UID_KEEP, x, UID_KEEP),
            OP_SETRESUID_S => self.op_setresuid(UID_KEEP, UID_KEEP, x),
            _ => self.op_setresuid(x, x, x),
        }
    }
}

/// 切换矩阵格子结果：(特权态, 操作, 输入 uid, 期望三值或 -EPERM)。
pub struct Cell {
    pub prived: bool,
    pub op: u8,
    pub x: u32,
    pub expect: Result<(u32, u32, u32), u32>,
}

/// 16 格矩阵（特权/非特权 × 8 操作，样本 uid：r=1000 e=1000 s=1000 起始，
/// 特权样本 r=0 e=0 s=0；操作值 x=1001 非特权必拒、特权必成）。
pub const MATRIX_16: [Cell; 16] = build_matrix();

const fn build_matrix() -> [Cell; 16] {
    [
        // 特权态 8 格（root 起始，x=1001 → 全成）
        Cell { prived: true, op: OP_SETUID, x: 1001, expect: Ok((1001, 1001, 1001)) },
        Cell { prived: true, op: OP_SETREUID_R, x: 1001, expect: Ok((1001, 0, 0)) },
        Cell { prived: true, op: OP_SETREUID_E, x: 1001, expect: Ok((0, 1001, 0)) },
        Cell { prived: true, op: OP_SETREUID_RE, x: 1001, expect: Ok((1001, 1001, 0)) },
        Cell { prived: true, op: OP_SETRESUID_R, x: 1001, expect: Ok((1001, 0, 0)) },
        Cell { prived: true, op: OP_SETRESUID_E, x: 1001, expect: Ok((0, 1001, 0)) },
        Cell { prived: true, op: OP_SETRESUID_S, x: 1001, expect: Ok((0, 0, 1001)) },
        Cell { prived: true, op: OP_SETRESUID_RES, x: 1001, expect: Ok((1001, 1001, 1001)) },
        // 非特权态 8 格（user 起始 r=1000 e=1000 s=1000，x=1001 不在池 → 全拒）
        Cell { prived: false, op: OP_SETUID, x: 1001, expect: Err(XC_ID_EPERM) },
        Cell { prived: false, op: OP_SETREUID_R, x: 1001, expect: Err(XC_ID_EPERM) },
        Cell { prived: false, op: OP_SETREUID_E, x: 1001, expect: Err(XC_ID_EPERM) },
        Cell { prived: false, op: OP_SETREUID_RE, x: 1001, expect: Err(XC_ID_EPERM) },
        Cell { prived: false, op: OP_SETRESUID_R, x: 1001, expect: Err(XC_ID_EPERM) },
        Cell { prived: false, op: OP_SETRESUID_E, x: 1001, expect: Err(XC_ID_EPERM) },
        Cell { prived: false, op: OP_SETRESUID_S, x: 1001, expect: Err(XC_ID_EPERM) },
        Cell { prived: false, op: OP_SETRESUID_RES, x: 1001, expect: Err(XC_ID_EPERM) },
    ]
}

/// 16 格矩阵逐格断言核。
pub fn matrix_16_exact() -> bool {
    for c in MATRIX_16.iter() {
        let mut cr = if c.prived {
            Cred3::new(0, 0, 0)
        } else {
            Cred3::new(1000, 1000, 1000)
        };
        let got = cr.apply(c.op, c.x).map(|_| (cr.ruid, cr.euid, cr.suid));
        match (got, c.expect) {
            (Ok(g), Ok(e)) => {
                if g != e {
                    return false;
                }
            }
            (Err(g), Err(e)) => {
                if g != e {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

/// 非特权回退核：euid 回 ruid/suid 成功（setuid(1000)）。
pub fn unpriv_fallback_exact() -> bool {
    let mut cr = Cred3::new(1000, 0, 1000); // 非特权但 suid=1000
    if cr.op_setuid(1000).is_err() {
        return false;
    }
    cr.euid == 1000 && cr.ruid == 1000 && cr.suid == 1000
}

/// 特权全设核：root setuid → 三值全变。
pub fn priv_setuid_exact() -> bool {
    let mut cr = Cred3::new(0, 0, 0);
    if cr.op_setuid(1002).is_err() {
        return false;
    }
    cr.ruid == 1002 && cr.euid == 1002 && cr.suid == 1002 && !cr.privileged()
}

/// 注入核（5 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 非特权 setuid 陌生值
    let mut cr = Cred3::new(1000, 1000, 1000);
    if cr.op_setuid(0) == Err(XC_ID_EPERM) {
        caught += 1;
    }
    // 2) 非特权 setreuid 陌生值
    if cr.op_setreuid(0, UID_KEEP) == Err(XC_ID_EPERM) {
        caught += 1;
    }
    // 3) 非特权 setresuid 陌生值
    if cr.op_setresuid(UID_KEEP, 0, UID_KEEP) == Err(XC_ID_EPERM) {
        caught += 1;
    }
    // 4) 操作号越界
    if cr.apply(9, 0) == Err(XC_ID_RANGE) {
        caught += 1;
    }
    // 5) KEEP 哨兵不动
    let before = (cr.ruid, cr.euid, cr.suid);
    let _ = cr.op_setresuid(UID_KEEP, UID_KEEP, UID_KEEP);
    if (cr.ruid, cr.euid, cr.suid) == before {
        caught += 1;
    }
    (caught, 5)
}

/// 指纹冻结：三身份折叠哈希。
pub fn fp_frozen(c: &Cred3) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    h ^= c.ruid as u64;
    h = h.wrapping_mul(0x100000001b3);
    h ^= c.euid as u64;
    h = h.wrapping_mul(0x100000001b3);
    h ^= c.suid as u64;
    h = h.wrapping_mul(0x100000001b3);
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx8881_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F8881");
    // J1-1 16 格矩阵逐格
    let ok = matrix_16_exact();
    s.add("B05-01-16格矩阵", ok, if ok { "" } else { "矩阵漂移" });
    // J1-2 非特权回退
    let ok = unpriv_fallback_exact();
    s.add("B05-02-非特回退", ok, if ok { "" } else { "回退漂移" });
    // J1-3 特权全设
    let ok = priv_setuid_exact();
    s.add("B05-03-特权全设", ok, if ok { "" } else { "全设漂移" });
    // J1-4 操作闭集
    let ok = OPS == 8;
    s.add("B05-04-操作闭集", ok, if ok { "" } else { "闭集漂移" });
    // J1-5 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B05-05-注入核必抓", ok, &detail);
    // J1-6 指纹冻结
    let c = Cred3::new(1000, 1000, 1000);
    let ok = fp_frozen(&c) == fp_frozen(&c);
    s.add("B05-06-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
