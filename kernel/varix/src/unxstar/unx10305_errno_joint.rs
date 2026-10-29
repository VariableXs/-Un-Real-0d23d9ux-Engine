//! UNX-F10305 errno 码面联签（lxerrno 码名一致）（0x1b0a0 段）
//!
//! 锚点：UNX-F10305-J1 码名一致——lxerrno 十二码逐码双方报名一致，
//! 异名即分歧可枚举，码面越界必拒。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：码名册（码→甲侧名/乙侧名双栏登记，码域 1..=32 越界拒）+
//! 一致性核（双名同串才合账，异名分歧单列）+ 封账核（封后写账即拒）+
//! 注入核（异名/越界码/未知码查询/封账后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b0a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b0a0;
pub const EJ_DUP: u32 = SEG_BASE + 0x01; // 码重号
pub const EJ_EMPTY: u32 = SEG_BASE + 0x02; // 名空
pub const EJ_RANGE: u32 = SEG_BASE + 0x03; // 码越域
pub const EJ_NOCODE: u32 = SEG_BASE + 0x04; // 未知码查询
pub const EJ_CAP: u32 = SEG_BASE + 0x05; // 册超容
pub const EJ_SEALED: u32 = SEG_BASE + 0x06; // 封账后写账

/// lxerrno 码域上界（含）。
pub const LX_ERRNO_MAX: u32 = 32;
/// 码名册定容。
pub const CAP: usize = 16;
/// canonical 十二码锚。
pub const CODES_NEEDED: usize = 12;

/// 单行码名：甲/乙两侧各报一名。
#[derive(Clone)]
pub struct ErrnoRow {
    pub code: u32,
    pub name_a: String,
    pub name_b: String,
}

impl ErrnoRow {
    /// 双名一致。
    pub fn agree(&self) -> bool {
        self.name_a == self.name_b
    }

    pub fn ok(&self) -> bool {
        self.code >= 1 && self.code <= LX_ERRNO_MAX && !self.name_a.is_empty() && !self.name_b.is_empty()
    }

    pub fn render(&self) -> String {
        let flag = if self.agree() { "=" } else { "!" };
        format!("{}{}{}", self.name_a, flag, self.name_b)
    }
}

/// lxerrno 码面联签册。
pub struct ErrnoJoint {
    pub rows: Vec<ErrnoRow>,
    pub sealed: bool,
}

impl ErrnoJoint {
    pub fn new() -> ErrnoJoint {
        ErrnoJoint { rows: Vec::new(), sealed: false }
    }

    fn pos(&self, code: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].code == code {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 登记码行：封账、越域、重号、双名任一空、超容均拒。
    pub fn register(&mut self, code: u32, na: &str, nb: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(EJ_SEALED);
        }
        if code == 0 || code > LX_ERRNO_MAX {
            return Err(EJ_RANGE);
        }
        if na.is_empty() || nb.is_empty() {
            return Err(EJ_EMPTY);
        }
        if self.pos(code).is_some() {
            return Err(EJ_DUP);
        }
        if self.rows.len() >= CAP {
            return Err(EJ_CAP);
        }
        self.rows.push(ErrnoRow { code, name_a: na.to_string(), name_b: nb.to_string() });
        Ok(())
    }

    /// 单码一致性裁断：未知码拒。
    pub fn agree(&self, code: u32) -> Result<bool, u32> {
        match self.pos(code) {
            None => Err(EJ_NOCODE),
            Some(k) => Ok(self.rows[k].agree()),
        }
    }

    /// 分歧码清单（按登记序）。
    pub fn disagreements(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.rows.len() {
            if !self.rows[k].agree() {
                v.push(self.rows[k].code);
            }
            k += 1;
        }
        v
    }

    /// 码 → 双名渲染（未知码拒）。
    pub fn render_of(&self, code: u32) -> Result<String, u32> {
        match self.pos(code) {
            None => Err(EJ_NOCODE),
            Some(k) => Ok(self.rows[k].render()),
        }
    }

    /// 码面合账：锚数齐、零分歧、全行合法。
    pub fn settled(&self) -> bool {
        self.rows.len() == CODES_NEEDED && self.disagreements().is_empty() && self.rows.iter().all(|r| r.ok())
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 码面折叠指纹（码序+双名字节）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b0a00;
        h = h.wrapping_mul(31).wrapping_add(self.rows.len() as u32);
        let mut k = 0usize;
        while k < self.rows.len() {
            let r = &self.rows[k];
            h = h.wrapping_mul(31).wrapping_add(r.code);
            let mut side = 0usize;
            while side < 2 {
                let bytes = if side == 0 { r.name_a.as_bytes() } else { r.name_b.as_bytes() };
                let mut b = 0usize;
                while b < bytes.len() {
                    h = h.wrapping_mul(31).wrapping_add(bytes[b] as u32);
                    b += 1;
                }
                side += 1;
            }
            k += 1;
        }
        h
    }
}

/// canonical 十二码码名样本（码, 名）。
pub const CANON_ERRNOS: [(u32, &str); CODES_NEEDED] = [
    (1, "EPERM"),
    (2, "ENOENT"),
    (4, "EINTR"),
    (9, "EBADF"),
    (11, "EAGAIN"),
    (12, "ENOMEM"),
    (13, "EACCES"),
    (17, "EEXIST"),
    (20, "ENOTDIR"),
    (21, "EISDIR"),
    (22, "EINVAL"),
    (28, "ENOSPC"),
];

/// 按 canonical 码名装配合账册。
pub fn build_canon() -> ErrnoJoint {
    let mut j = ErrnoJoint::new();
    let mut k = 0usize;
    while k < CANON_ERRNOS.len() {
        let _ = j.register(CANON_ERRNOS[k].0, CANON_ERRNOS[k].1, CANON_ERRNOS[k].1);
        k += 1;
    }
    j
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 异名（乙侧报错名）必被分歧面抓出
    let mut j = ErrnoJoint::new();
    let _ = j.register(22, "EINVAL", "EINVL");
    got[0] = j.agree(22) == Ok(false) && j.disagreements() == [22u32];
    // 2) 码越域（0 与 33）必拒
    got[1] = j.register(0, "A", "A").err() == Some(EJ_RANGE)
        && j.register(33, "B", "B").err() == Some(EJ_RANGE);
    // 3) 未知码查询必拒
    got[2] = j.agree(9).err() == Some(EJ_NOCODE);
    // 4) 封账后写账必抓
    j.seal();
    got[3] = j.register(9, "EBADF", "EBADF").err() == Some(EJ_SEALED);
    got
}

/// 批内判据。
pub fn run_unx10305_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10305");

    // J1 正路径：十二码逐码双名一致合账
    let book = build_canon();
    s.add("J1-十二码合账", book.settled(), "canonical 码面零分歧且行全合法");

    // J2 逐码一致性裁断
    let j2 = {
        let mut ok = true;
        let mut k = 0usize;
        while k < CANON_ERRNOS.len() {
            if book.agree(CANON_ERRNOS[k].0) != Ok(true) {
                ok = false;
            }
            k += 1;
        }
        ok
    };
    s.add("J2-逐码一致", j2, "十二码逐码裁一致");

    // J3 注入检出：异名 + 越域
    let inj = inject_four();
    s.add("J3-异名越域检出", inj[0] && inj[1], "异名必抓且越域必拒");

    // J4 注入检出：未知码 + 封账后写
    s.add("J4-未知封账检出", inj[2] && inj[3], "未知码必拒且封账后写必抓");

    // J5 重号拒
    let j5 = {
        let mut c = ErrnoJoint::new();
        let _ = c.register(22, "EINVAL", "EINVAL");
        c.register(22, "EINVAL", "EINVAL").err() == Some(EJ_DUP)
    };
    s.add("J5-重号拒收", j5, "码重号必拒");

    // J6 双名任一空拒
    let j6 = ErrnoJoint::new().register(1, "EPERM", "").err() == Some(EJ_EMPTY)
        && ErrnoJoint::new().register(1, "", "EPERM").err() == Some(EJ_EMPTY);
    s.add("J6-空名拒收", j6, "任一侧名空必拒");

    // J7 分歧枚举有序：两条分歧按登记序
    let j7 = {
        let mut c = ErrnoJoint::new();
        let _ = c.register(4, "EINTR", "EINTRX");
        let _ = c.register(9, "EBADF", "EBADFX");
        c.disagreements() == [4u32, 9u32]
    };
    s.add("J7-分歧枚举", j7, "分歧码按登记序枚举");

    // J8 容量边界：册满 16 后域内合法码（17）按超容拒
    let j8 = {
        let mut c = ErrnoJoint::new();
        let mut filled = true;
        let mut m = 1u32;
        while m <= CAP as u32 {
            if c.register(m, "x", "x").is_err() {
                filled = false;
            }
            m += 1;
        }
        filled && c.register(17, "y", "y").err() == Some(EJ_CAP)
    };
    s.add("J8-容量边界", j8, "册满 16 后越域样本必拒");

    // J9 码域上界恰可登记（code==32 合法）
    let j9 = {
        let mut c = ErrnoJoint::new();
        c.register(32, "ELAST", "ELAST").is_ok()
    };
    s.add("J9-域界合法", j9, "码恰为上界仍可登记");

    // J10 渲染分旗：一致/分歧可区分
    let j10 = {
        let a = ErrnoRow { code: 1, name_a: "A".to_string(), name_b: "A".to_string() };
        let b = ErrnoRow { code: 2, name_a: "B".to_string(), name_b: "C".to_string() };
        a.render() != b.render() && a.render().contains("=") && b.render().contains("!")
    };
    s.add("J10-渲染分旗", j10, "一致与分歧渲染可区分");

    // J11 封账指纹稳定
    let j11 = {
        let mut c = build_canon();
        let f0 = c.fold();
        c.seal();
        c.fold() == f0 && c.sealed
    };
    s.add("J11-封账指纹", j11, "封账不翻指纹且封单位翻转");

    // J12 动态明细：分歧零清单
    let dz = book.disagreements().len();
    s.add_dyn("J12-分歧零明细", dz == 0, &format!("disagree={}", dz));

    // J13 动态明细：码面指纹落账
    let fp = book.fold();
    s.add_dyn("J13-码面指纹", fp != 0, &format!("fold={:#x}", fp));

    // J14 动态明细：码名渲染对账
    let r22 = book.render_of(22);
    let j14 = r22 == Ok("EINVAL=EINVAL".to_string())
        && book.render_of(9) == Ok("EBADF=EBADF".to_string())
        && book.render_of(99).err() == Some(EJ_NOCODE);
    s.add_dyn("J14-码名渲染", j14, &format!("码 22 渲染 {:?}", r22));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10305_errno_flow() {
        let mut j = ErrnoJoint::new();
        assert!(j.register(22, "EINVAL", "EINVAL").is_ok());
        assert!(j.register(22, "X", "X").err() == Some(EJ_DUP));
        assert!(j.register(0, "A", "A").err() == Some(EJ_RANGE));
        assert!(j.register(33, "B", "B").err() == Some(EJ_RANGE));
        assert!(j.register(9, "EBADF", "").err() == Some(EJ_EMPTY));
        assert_eq!(j.agree(22), Ok(true));
        assert!(j.agree(9).err() == Some(EJ_NOCODE));
        j.seal();
        assert!(j.register(9, "EBADF", "EBADF").err() == Some(EJ_SEALED));
    }

    #[test]
    fn unx10305_checks_green() {
        let s = run_unx10305_checks();
        assert!(s.all_passed(), "UNX-F10305 判据未全绿");
    }
}
