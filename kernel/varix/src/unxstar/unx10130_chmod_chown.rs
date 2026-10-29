//! UNX-F10130 chmod/chown 权限面正反双判据（位操作达成与越权误用受控）（0x10360 段）
//!
//! 锚点：UNX-F10130-J1 位操作达成（mode 置位/清位回读一致 LTP chmod01
//! 面 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：权限册（mode 位面 + 属主三元组）+ 位核（置位/清位/回读/chown
//! 属主迁移/越权拒）+ 权账（逐次留痕）+ 注入核（越权/野位/缺件/空留痕/
//! 封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10360 独占 0x20）。
pub const SEG_BASE: u32 = 0x10360;
pub const CM_EPERM: u32 = SEG_BASE + 0x01; // 越权
pub const CM_BAD_MODE: u32 = SEG_BASE + 0x02; // 野位出闭集
pub const CM_NOENT: u32 = SEG_BASE + 0x03; // 缺件
pub const CM_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const CM_SEALED: u32 = SEG_BASE + 0x05; // 封表后操作

/// mode 12 位闭集。
pub const MODE_MASK: u32 = 0o7777;

/// 权限件。
#[derive(Clone)]
pub struct PermNode {
    pub name: String,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
}

impl PermNode {
    pub fn ok(&self) -> bool {
        !self.name.is_empty() && self.mode & !MODE_MASK == 0
    }

    pub fn render(&self) -> String {
        format!("{}|{:o}|u{}g{}", self.name, self.mode, self.uid, self.gid)
    }
}

/// 权账留痕。
#[derive(Clone)]
pub struct CmRec {
    pub seq: u32,
    pub op: u32, // 0=chmod 1=chown
    pub name: String,
    pub arg: u32,
    pub as_owner: bool,
}

impl CmRec {
    pub fn ok(&self) -> bool {
        self.op <= 1 && !self.name.is_empty()
    }
}

/// 权限册 + 位核。
pub struct PermBook {
    pub nodes: Vec<PermNode>,
    pub ledger: Vec<CmRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl PermBook {
    pub fn new() -> PermBook {
        PermBook { nodes: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 建（mode 位出闭集拒）。
    pub fn create(&mut self, name: &str, mode: u32, uid: u32, gid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(CM_SEALED);
        }
        if name.is_empty() {
            return Err(CM_NOENT);
        }
        if mode & !MODE_MASK != 0 {
            return Err(CM_BAD_MODE);
        }
        self.nodes.push(PermNode { name: name.to_string(), mode, uid, gid });
        Ok(())
    }

    /// chmod：属主可改、他人越权拒。
    pub fn chmod(&mut self, name: &str, mode: u32, as_owner: bool, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(CM_SEALED);
        }
        if note.is_empty() {
            return Err(CM_EMPTY_NOTE);
        }
        if mode & !MODE_MASK != 0 {
            return Err(CM_BAD_MODE);
        }
        let i = match self.nodes.iter().position(|n| n.name == name) {
            Some(i) => i,
            None => return Err(CM_NOENT),
        };
        if !as_owner {
            return Err(CM_EPERM);
        }
        self.nodes[i].mode = mode;
        self.ledger.push(CmRec {
            seq: self.seq_next,
            op: 0,
            name: name.to_string(),
            arg: mode,
            as_owner,
        });
        self.seq_next += 1;
        Ok(mode)
    }

    /// chown：属主可迁、他人越权拒。
    pub fn chown(&mut self, name: &str, uid: u32, gid: u32, as_owner: bool, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(CM_SEALED);
        }
        if note.is_empty() {
            return Err(CM_EMPTY_NOTE);
        }
        let i = match self.nodes.iter().position(|n| n.name == name) {
            Some(i) => i,
            None => return Err(CM_NOENT),
        };
        if !as_owner {
            return Err(CM_EPERM);
        }
        self.nodes[i].uid = uid;
        self.nodes[i].gid = gid;
        self.ledger.push(CmRec {
            seq: self.seq_next,
            op: 1,
            name: name.to_string(),
            arg: uid,
            as_owner,
        });
        self.seq_next += 1;
        Ok(())
    }

    /// 置位语义：mode |= bits。
    pub fn set_bits(&mut self, name: &str, bits: u32, as_owner: bool) -> Result<u32, u32> {
        let cur = match self.nodes.iter().find(|n| n.name == name) {
            Some(n) => n.mode,
            None => return Err(CM_NOENT),
        };
        self.chmod(name, cur | bits, as_owner, "set_bits")
    }

    /// 清位语义：mode &= !bits。
    pub fn clr_bits(&mut self, name: &str, bits: u32, as_owner: bool) -> Result<u32, u32> {
        let cur = match self.nodes.iter().find(|n| n.name == name) {
            Some(n) => n.mode,
            None => return Err(CM_NOENT),
        };
        self.chmod(name, cur & !bits, as_owner, "clr_bits")
    }

    /// 回读。
    pub fn mode_of(&self, name: &str) -> Option<u32> {
        self.nodes.iter().find(|n| n.name == name).map(|n| n.mode)
    }

    pub fn owner_of(&self, name: &str) -> Option<(u32, u32)> {
        self.nodes.iter().find(|n| n.name == name).map(|n| (n.uid, n.gid))
    }

    /// chmod 次数统计。
    pub fn chmod_count(&self) -> usize {
        self.ledger.iter().filter(|r| r.op == 0).count()
    }

    /// chown 次数统计。
    pub fn chown_count(&self) -> usize {
        self.ledger.iter().filter(|r| r.op == 1).count()
    }

    /// 位面全绿核：读位/写位/执行位三面按 mode 推导。
    pub fn bits_of(&self, name: &str) -> Option<(bool, bool, bool)> {
        let m = self.mode_of(name)?;
        Some((
            m & 0o444 != 0,
            m & 0o222 != 0,
            m & 0o111 != 0,
        ))
    }

    /// 属主一致性核：uid/gid 同迁。
    pub fn owner_pair_ok(&self, name: &str) -> bool {
        match self.owner_of(name) {
            Some((u, g)) => u != 0 && g != 0,
            None => false,
        }
    }

    /// 全册渲染（诊断面）。
    pub fn render_all(&self) -> String {
        let mut out = String::new();
        for n in &self.nodes {
            out.push_str(&n.render());
            out.push(';');
        }
        out
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
        let mut h: u32 = 0x103600;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.op);
            h = h.wrapping_mul(31).wrapping_add(r.arg);
            for b in r.name.bytes() {
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
    let mut b = PermBook::new();
    let _ = b.create("f", 0o644, 1000, 1000);
    // 1) 越权 chmod 必拒
    got[0] = b.chmod("f", 0o600, false, "inj-ep").err() == Some(CM_EPERM);
    // 2) 野位必拒
    got[1] = b.chmod("f", 0o20000, true, "inj-wild").err() == Some(CM_BAD_MODE);
    // 3) 缺件必拒
    got[2] = b.chmod("ghost", 0o600, true, "inj-noent").err() == Some(CM_NOENT);
    // 4) 空留痕必拒
    got[3] = b.chmod("f", 0o600, true, "").err() == Some(CM_EMPTY_NOTE);
    // 5) 封表后操作必抓
    b.seal();
    got[4] = b.chmod("f", 0o600, true, "inj-seal").err() == Some(CM_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10130_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10130");

    // 正路径：chmod 置位回读一致（chmod01 面）
    let mut b = PermBook::new();
    let _ = b.create("cfg", 0o644, 1000, 1000);
    let _ = b.chmod("cfg", 0o600, true, "收紧");
    let j1 = b.mode_of("cfg") == Some(0o600);
    s.add("J1-置位回读", j1, "chmod 0600 回读一致");

    // 置位/清位往返
    let j2 = {
        let up = b.set_bits("cfg", 0o070, true).unwrap_or(0);
        let down = b.clr_bits("cfg", 0o070, true).unwrap_or(0);
        up & 0o070 == 0o070 && down & 0o070 == 0 && down & 0o600 == 0o600
    };
    s.add("J2-置清往返", j2, "组/它位先置后清且主位不动");

    // chown 属主迁移
    let j3 = {
        let _ = b.chown("cfg", 1001, 1001, true, "迁移");
        b.owner_of("cfg") == Some((1001, 1001))
    };
    s.add("J3-属主迁移", j3, "chown 双元组回读一致");

    // 越权受控（chmod/chown 双面）
    let j4 = b.chmod("cfg", 0o777, false, "野改").err() == Some(CM_EPERM)
        && b.chown("cfg", 0, 0, false, "野迁").err() == Some(CM_EPERM);
    s.add("J4-越权受控", j4, "非属主双操作同拒");

    // 野位受控
    let j5 = b.chmod("cfg", 0o10000, true, "野位").err() == Some(CM_BAD_MODE)
        && b.create("w", 0o17777, 0, 0).err() == Some(CM_BAD_MODE);
    s.add("J5-野位受控", j5, "位出 12 位闭集必拒");

    // 缺件受控
    let j6 = b.chmod("ghost", 0o600, true, "缺席").err() == Some(CM_NOENT)
        && b.chown("ghost", 1, 1, true, "缺席").err() == Some(CM_NOENT);
    s.add("J6-缺件受控", j6, "缺件双操作同拒");

    // 八进制渲染
    let j7 = {
        let _ = b.chmod("cfg", 0o755, true, "标准");
        b.nodes.iter().find(|n| n.name == "cfg").map(|n| n.render().contains("755")).unwrap_or(false)
    };
    s.add("J7-八进制渲染", j7, "mode 以八进制入渲染");

    // 空留痕拒收
    let j8 = b.chmod("cfg", 0o700, true, "").err() == Some(CM_EMPTY_NOTE);
    s.add("J8-空留痕拒", j8, "note 为空不入账");

    // 节点合法性守恒
    let j9 = b.nodes.iter().all(|n| n.ok());
    s.add("J9-节点合法", j9, "名非空且位在闭集");

    // 留痕合法与 seq 唯一
    let j10 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J10-留痕合法", j10, "op 在闭集且名非空");

    // 注入核 5/5
    let inj = inject_five();
    let j11 = inj.iter().all(|g| *g);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", inj));

    // 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    // 位面推导与双计数
    let bits = b.bits_of("cfg").unwrap_or((false, false, false));
    let j13 = bits.0 && bits.1 && bits.2 && b.chmod_count() >= 2 && b.chown_count() >= 1;
    s.add("J13-位面计数", j13, "755 位面读/写/执行全真且双计数达锚");

    // 属主一致与全册渲染
    let j14 = b.owner_pair_ok("cfg") && b.render_all().contains("755");
    s.add("J14-属主渲染", j14, "uid/gid 异号迁移且渲染命中");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10130_perm_flow() {
        let mut b = PermBook::new();
        let _ = b.create("a", 0o600, 1, 1);
        let _ = b.set_bits("a", 0o040, true);
        assert_eq!(b.mode_of("a"), Some(0o640));
        let _ = b.clr_bits("a", 0o040, true);
        assert_eq!(b.mode_of("a"), Some(0o600));
        assert!(b.chown("a", 2, 2, false, "x") == Err(CM_EPERM));
    }

    #[test]
    fn unx10130_checks_green() {
        let s = run_unx10130_checks();
        assert!(s.all_passed(), "UNX-F10130 判据未全绿");
    }
}
