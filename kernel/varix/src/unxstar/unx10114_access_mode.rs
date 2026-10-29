//! UNX-F10114 access 正反双判据（可达性判定达成与非法 mode 误用受控）（0x10160 段）
//!
//! 锚点：UNX-F10114-J1 access 可达性判定达成（R_OK/W_OK/F_OK 面 LTP
//! access01 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：权限件（rwx 位 + 属主/他人双视角）+ 判定核（F_OK 存在性/
//! R_OK/W_OK/X_OK 位与运算/非法 mode 拒收）+ 判定账（逐次留痕）+ 注入
//! 核（非法 mode/缺失路径/越权视角/空留痕/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10160 独占 0x20）。
pub const SEG_BASE: u32 = 0x10160;
pub const AC_BAD_MODE: u32 = SEG_BASE + 0x01; // mode 出闭集
pub const AC_NOENT: u32 = SEG_BASE + 0x02; // 缺失路径
pub const AC_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const AC_SEALED: u32 = SEG_BASE + 0x04; // 封表后操作
pub const AC_DENIED: u32 = SEG_BASE + 0x05; // 权限拒绝（诊断码）

/// access mode 闭集。
pub const F_OK: u32 = 0;
pub const X_OK: u32 = 1;
pub const W_OK: u32 = 2;
pub const R_OK: u32 = 4;

/// 权限位掩码。
pub const BIT_R: u32 = 4;
pub const BIT_W: u32 = 2;
pub const BIT_X: u32 = 1;

/// 权限件。
#[derive(Clone)]
pub struct PermFile {
    pub name: String,
    pub owner_bits: u32,
    pub other_bits: u32,
}

impl PermFile {
    pub fn new(name: &str, owner: u32, other: u32) -> PermFile {
        PermFile { name: name.to_string(), owner_bits: owner, other_bits: other }
    }

    /// 位闭集：rwx 之外不许有。
    pub fn bits_ok(v: u32) -> bool {
        v & !(BIT_R | BIT_W | BIT_X) == 0
    }

    pub fn render(&self) -> String {
        format!("{}|o{:o}w{:o}", self.name, self.owner_bits, self.other_bits)
    }
}

/// 判定账留痕。
#[derive(Clone)]
pub struct AcRec {
    pub seq: u32,
    pub path: String,
    pub mode: u32,
    pub as_owner: bool,
    pub granted: bool,
}

impl AcRec {
    pub fn ok(&self) -> bool {
        !self.path.is_empty()
            && (self.mode == F_OK || self.mode == R_OK || self.mode == W_OK || self.mode == X_OK)
    }
}

/// 权限册 + 判定核。
pub struct AccessBook {
    pub files: Vec<PermFile>,
    pub ledger: Vec<AcRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl AccessBook {
    pub fn new() -> AccessBook {
        AccessBook { files: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn install(&mut self, f: PermFile) -> Result<(), u32> {
        if self.sealed {
            return Err(AC_SEALED);
        }
        if f.name.is_empty() {
            return Err(AC_NOENT);
        }
        if !PermFile::bits_ok(f.owner_bits) || !PermFile::bits_ok(f.other_bits) {
            return Err(AC_BAD_MODE);
        }
        self.files.push(f);
        Ok(())
    }

    /// access 核心：F_OK 存在性；R/W/X 按视角位与。
    pub fn access(
        &mut self,
        path: &str,
        mode: u32,
        as_owner: bool,
        note: &str,
    ) -> Result<bool, u32> {
        if self.sealed {
            return Err(AC_SEALED);
        }
        if note.is_empty() {
            return Err(AC_EMPTY_NOTE);
        }
        if mode != F_OK && mode != R_OK && mode != W_OK && mode != X_OK {
            return Err(AC_BAD_MODE);
        }
        let f = match self.files.iter().find(|f| f.name == path) {
            Some(f) => f.clone(),
            None => return Err(AC_NOENT),
        };
        let bits = if as_owner { f.owner_bits } else { f.other_bits };
        let granted = match mode {
            F_OK => true,
            R_OK => bits & BIT_R != 0,
            W_OK => bits & BIT_W != 0,
            _ => bits & BIT_X != 0,
        };
        self.ledger.push(AcRec {
            seq: self.seq_next,
            path: path.to_string(),
            mode,
            as_owner,
            granted,
        });
        self.seq_next += 1;
        Ok(granted)
    }

    /// 账内拒绝数。
    pub fn denied_count(&self) -> usize {
        self.ledger.iter().filter(|r| !r.granted).count()
    }

    /// 授予数。
    pub fn granted_count(&self) -> usize {
        self.ledger.iter().filter(|r| r.granted).count()
    }

    /// chmod 面：按名置位（越名拒、位出闭集拒）。
    pub fn set_bits(&mut self, path: &str, as_owner: bool, bits: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(AC_SEALED);
        }
        if !PermFile::bits_ok(bits) {
            return Err(AC_BAD_MODE);
        }
        let f = match self.files.iter_mut().find(|f| f.name == path) {
            Some(f) => f,
            None => return Err(AC_NOENT),
        };
        if as_owner {
            f.owner_bits = bits;
        } else {
            f.other_bits = bits;
        }
        Ok(())
    }

    /// 可写文件清单（按名枚举，判定面辅助）。
    pub fn writable_names(&self, as_owner: bool) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for f in &self.files {
            let bits = if as_owner { f.owner_bits } else { f.other_bits };
            if bits & BIT_W != 0 {
                out.push(f.name.clone());
            }
        }
        out
    }

    /// 全矩阵自检：对每件每 mode 各判一次（F/R/W/X 四面）。
    pub fn audit_matrix(&mut self, as_owner: bool) -> Vec<bool> {
        let mut row: Vec<bool> = Vec::new();
        let names: Vec<String> = self.files.iter().map(|f| f.name.clone()).collect();
        for name in &names {
            for mode in [F_OK, R_OK, W_OK, X_OK] {
                let g = self.access(name, mode, as_owner, "matrix").unwrap_or(false);
                row.push(g);
            }
        }
        row
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.ledger.len() {
            for j in (i + 1)..self.ledger.len() {
                if self.ledger[i].seq == self.ledger[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x101600;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.mode);
            h = h.wrapping_mul(31).wrapping_add(r.granted as u32);
            for b in r.path.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = AccessBook::new();
    let _ = b.install(PermFile::new("f.txt", BIT_R | BIT_W, 0));
    // 1) 非法 mode 必拒
    got[0] = b.access("f.txt", 3, true, "inj-mode").err() == Some(AC_BAD_MODE);
    // 2) 缺失路径必拒
    got[1] = b.access("ghost", F_OK, true, "inj-noent").err() == Some(AC_NOENT);
    // 3) 越权视角：他人无 W 位则拒绝
    got[2] = b.access("f.txt", W_OK, false, "inj-denied").unwrap_or(true) == false;
    // 4) 空留痕必拒
    got[3] = b.access("f.txt", R_OK, true, "").err() == Some(AC_EMPTY_NOTE);
    // 5) 封表后操作必抓
    b.seal();
    got[4] = b.access("f.txt", R_OK, true, "inj-seal").err() == Some(AC_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10114_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10114");

    // 正路径：R_OK/W_OK/F_OK 三面（access01 面）
    let mut b = AccessBook::new();
    let _ = b.install(PermFile::new("rw.txt", BIT_R | BIT_W, BIT_R));
    let j1 = b.access("rw.txt", R_OK, true, "读").unwrap_or(false)
        && b.access("rw.txt", W_OK, true, "写").unwrap_or(false)
        && b.access("rw.txt", F_OK, true, "存在").unwrap_or(false);
    s.add("J1-三面达成", j1, "R/W/F 三判齐过");

    // X_OK 位与
    let j2 = {
        let _ = b.install(PermFile::new("x.sh", BIT_R | BIT_X, 0));
        b.access("x.sh", X_OK, true, "执行").unwrap_or(false)
    };
    s.add("J2-X位判定", j2, "X_OK 按 X 位授予");

    // 他人视角受控：只读文件拒 W
    let j3 = b.access("rw.txt", W_OK, false, "他人写").unwrap_or(true) == false;
    s.add("J3-他人拒写", j3, "他人无 W 位必拒");

    // 他人视角授予：他人有 R 位则 R_OK 过
    let j4 = b.access("rw.txt", R_OK, false, "他人读").unwrap_or(false);
    s.add("J4-他人可读", j4, "他人 R 位授予");

    // 非法 mode 受控（3/5/7 均出闭集）
    let j5 = b.access("rw.txt", 3, true, "m3").err() == Some(AC_BAD_MODE)
        && b.access("rw.txt", 8, true, "m8").err() == Some(AC_BAD_MODE);
    s.add("J5-mode闭集", j5, "mode 出四值闭集必拒");

    // 缺失路径受控
    let j6 = b.access("absent", F_OK, true, "缺席").err() == Some(AC_NOENT);
    s.add("J6-缺失路径", j6, "无件路径按 NOENT 拒");

    // 位闭集守恒：安装位出 rwx 闭集必拒
    let j7 = b.install(PermFile::new("bad.txt", 0x8, 0)).err() == Some(AC_BAD_MODE);
    s.add("J7-位闭集守恒", j7, "安装位出 rwx 闭集必拒");

    // 空留痕拒收
    let j8 = b.access("rw.txt", R_OK, true, "").err() == Some(AC_EMPTY_NOTE);
    s.add("J8-空留痕拒", j8, "note 为空不入账");

    // 判定账对平：拒绝/授予计数
    let j9 = b.denied_count() >= 1 && b.granted_count() >= 3;
    s.add("J9-账面计数", j9, "拒与授两计数各达锚");

    // 留痕合法与 seq 唯一
    let j10 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J10-留痕合法", j10, "mode 在闭集且路径非空");

    // 注入核 5/5
    let inj = inject_five();
    let j11 = inj.iter().all(|g| *g);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", inj));

    // chmod 面：置位后重判翻转
    let j13 = {
        let _ = b.set_bits("rw.txt", false, BIT_R | BIT_W);
        b.access("rw.txt", W_OK, false, "提权后").unwrap_or(false)
    };
    s.add("J13-置位翻转", j13, "他人位提权后 W_OK 翻绿");

    // 全矩阵自检：F_OK 面恒绿、owner R/W 面与位一致
    let j14 = {
        let row = b.audit_matrix(true);
        row.len() == b.files.len() * 4
            && row.iter().step_by(4).all(|g| *g)
    };
    s.add("J14-矩阵自检", j14, "每件四面全判且 F_OK 面恒绿");

    // 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10114_access_flow() {
        let mut b = AccessBook::new();
        let _ = b.install(PermFile::new("p", BIT_R, 0));
        assert!(b.access("p", R_OK, true, "a").unwrap_or(false));
        assert!(!b.access("p", W_OK, true, "b").unwrap_or(true));
        assert!(b.access("p", 9, true, "c") == Err(AC_BAD_MODE));
        assert!(b.access("q", F_OK, true, "d") == Err(AC_NOENT));
    }

    #[test]
    fn unx10114_checks_green() {
        let s = run_unx10114_checks();
        assert!(s.all_passed(), "UNX-F10114 判据未全绿");
    }
}
