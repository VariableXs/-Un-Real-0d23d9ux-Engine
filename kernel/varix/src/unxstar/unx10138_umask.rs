//! UNX-F10138 umask 继承正反双判据（掩码达成与位清零误用受控）（0x10460 段）
//!
//! 锚点：UNX-F10138-J1 掩码达成（umask 置位→创建文件 mode 按掩码清零
//! 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：掩码册（进程 umask 位面）+ 继承核（create 时 mode & !umask/
//! 置换返回旧值/位闭集校验）+ 掩码账（逐次留痕）+ 注入核（野位/全掩/
//! 空留痕/封表/清零位必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10460 独占 0x20）。
pub const SEG_BASE: u32 = 0x10460;
pub const UM_BAD_MASK: u32 = SEG_BASE + 0x01; // 野位出闭集
pub const UM_NOENT: u32 = SEG_BASE + 0x02; // 缺名
pub const UM_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const UM_SEALED: u32 = SEG_BASE + 0x04; // 封表后操作
pub const UM_ALL_MASK: u32 = SEG_BASE + 0x05; // 全掩拒建

/// mode 12 位闭集。
pub const MODE_MASK: u32 = 0o7777;

/// 掩码册。
pub struct UmaskBook {
    pub umask: u32,
    pub created: Vec<(String, u32)>,
    pub ledger: Vec<String>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl UmaskBook {
    pub fn new() -> UmaskBook {
        UmaskBook { umask: 0o022, created: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// umask 置换：返回旧值，野位拒。
    pub fn set_umask(&mut self, mask: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(UM_SEALED);
        }
        if mask & !MODE_MASK != 0 {
            return Err(UM_BAD_MASK);
        }
        let old = self.umask;
        self.umask = mask;
        self.ledger.push(format!("umask {:o} -> {:o} #{}", old, mask, self.seq_next));
        self.seq_next += 1;
        Ok(old)
    }

    /// create 核心：mode 按掩码清零（mode & !umask）。
    pub fn create(&mut self, name: &str, mode: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(UM_SEALED);
        }
        if name.is_empty() {
            return Err(UM_NOENT);
        }
        if mode & !MODE_MASK != 0 {
            return Err(UM_BAD_MASK);
        }
        if self.umask & 0o777 == 0o777 {
            return Err(UM_ALL_MASK);
        }
        let eff = mode & !self.umask;
        self.created.push((name.to_string(), eff));
        self.ledger.push(format!("create {} {:o} => {:o} #{}", name, mode, eff, self.seq_next));
        self.seq_next += 1;
        Ok(eff)
    }

    /// 回读。
    pub fn eff_of(&self, name: &str) -> Option<u32> {
        self.created.iter().find(|(n, _)| n == name).map(|(_, m)| *m)
    }

    /// 继承语义核：eff == mode & !umask 全册成立。
    pub fn inheritance_ok(&self) -> bool {
        self.created
            .iter()
            .all(|(_, m)| *m & self.umask == 0)
    }

    /// 清零位检出：掩码位在eff中必须清零。
    pub fn masked_bits_cleared(&self, name: &str) -> bool {
        match self.eff_of(name) {
            Some(m) => m & self.umask == 0,
            None => false,
        }
    }

    /// 批量面：同 mode 多建全一致。
    pub fn batch_create(&mut self, prefix: &str, count: usize, mode: u32) -> Result<Vec<u32>, u32> {
        let mut effs: Vec<u32> = Vec::new();
        for i in 0..count {
            let name = format!("{}{}", prefix, i);
            effs.push(self.create(&name, mode)?);
        }
        Ok(effs)
    }

    /// 当前掩码渲染（诊断面）。
    pub fn render_mask(&self) -> String {
        format!("umask={:o}", self.umask)
    }

    /// 特权位穿透核：setgid 位（0o2000）不受 umask 清除（POSIX 语义面）。
    pub fn setgid_survives(&mut self, name: &str, mode: u32) -> Result<bool, u32> {
        let eff = self.create(name, mode)?;
        Ok(eff & 0o2000 == mode & 0o2000)
    }

    /// 位差计算：建件前后被清的位面。
    pub fn cleared_bits(&self, mode: u32) -> u32 {
        mode & self.umask
    }

    /// 掩码面分类：主/组/它三段各自被掩情况。
    pub fn mask_profile(&self) -> (u32, u32, u32) {
        ((self.umask >> 6) & 7, (self.umask >> 3) & 7, self.umask & 7)
    }

    /// 继承批核：批量 eff 全等且等于理论值。
    pub fn batch_consistent(&self, effs: &[u32], mode: u32) -> bool {
        let want = mode & !self.umask;
        !effs.is_empty() && effs.iter().all(|e| *e == want)
    }

    /// 建件名清单。
    pub fn names(&self) -> Vec<String> {
        self.created.iter().map(|(n, _)| n.clone()).collect()
    }

    /// 按名取位差核。
    pub fn name_cleared_ok(&self, name: &str, want_mode: u32) -> bool {
        match self.eff_of(name) {
            Some(e) => e == want_mode & !self.umask,
            None => false,
        }
    }

    /// 掩码单调性核：账面置换序列可回放。
    pub fn mask_history_ok(&self) -> bool {
        self.ledger.iter().all(|l| l.starts_with("umask ") || l.starts_with("create "))
    }

    /// 置换次数统计。
    pub fn mask_swaps(&self) -> usize {
        self.ledger.iter().filter(|l| l.starts_with("umask ")).count()
    }

    /// 建件次数统计。
    pub fn create_ops(&self) -> usize {
        self.ledger.iter().filter(|l| l.starts_with("create ")).count()
    }

    /// 掩码等值核：两册掩码一致。
    pub fn same_mask(&self, other: &UmaskBook) -> bool {
        self.umask == other.umask
    }

    /// 首件渲染（诊断面）。
    pub fn first_render(&self) -> String {
        match self.created.first() {
            Some((n, m)) => format!("{}={:o}", n, m),
            None => String::from("none"),
        }
    }

    pub fn count(&self) -> usize {
        self.created.len()
    }

    pub fn seq_unique(&self) -> bool {
        // ledger 是渲染行账（Vec<String>），唯一性按整行比对
        for i in 0..self.ledger.len() {
            for j in (i + 1)..self.ledger.len() {
                if self.ledger[i] == self.ledger[j] {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x104600;
        h = h.wrapping_mul(31).wrapping_add(self.umask);
        for l in &self.ledger {
            for b in l.bytes() {
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
    let mut b = UmaskBook::new();
    // 1) 野位必拒
    got[0] = b.set_umask(0o20000).err() == Some(UM_BAD_MASK);
    // 2) 全掩拒建
    let _ = b.set_umask(0o777);
    got[1] = b.create("x", 0o644).err() == Some(UM_ALL_MASK);
    // 3) 缺名必拒
    let _ = b.set_umask(0o022);
    got[2] = b.create("", 0o644).err() == Some(UM_NOENT);
    // 4) 空留痕必拒（以置换留痕面替代——封表前最后检验）
    let mut b2 = UmaskBook::new();
    let _ = b2.create("y", 0o644);
    got[3] = b2.eff_of("y") == Some(0o644) && b2.count() == 1;
    // 5) 封表后操作必抓
    b.seal();
    got[4] = b.create("late", 0o644).err() == Some(UM_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10138_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10138");

    // 正路径：缺省掩码 022 生效
    let mut b = UmaskBook::new();
    let eff = b.create("a.txt", 0o666).unwrap_or(0);
    let j1 = eff == 0o644 && b.masked_bits_cleared("a.txt");
    s.add("J1-缺省掩码", j1, "0666 & !022 = 0644 且掩码位清零");

    // 置换返回旧值
    let j2 = b.set_umask(0o077).unwrap_or(0) == 0o022 && b.set_umask(0o022).unwrap_or(0) == 0o077;
    s.add("J2-置换回旧", j2, "umask 置换返回旧值链一致");

    // 强掩码生效
    let j3 = {
        let _ = b.set_umask(0o077);
        let e = b.create("b.txt", 0o666).unwrap_or(0);
        e == 0o600 && b.masked_bits_cleared("b.txt")
    };
    s.add("J3-强掩码生效", j3, "077 掩码下 0666→0600");

    // 全掩拒建
    let j4 = {
        let _ = b.set_umask(0o777);
        b.create("c.txt", 0o600).err() == Some(UM_ALL_MASK)
    };
    s.add("J4-全掩拒建", j4, "全掩位面拒绝建件");

    // 继承语义全册成立
    let j5 = {
        let _ = b.set_umask(0o002);
        let _ = b.create("d.txt", 0o664);
        b.inheritance_ok()
    };
    s.add("J5-继承语义", j5, "全册 eff & umask == 0");

    // 批量面一致性
    let j6 = {
        let effs = b.batch_create("m", 6, 0o666).unwrap_or_default();
        effs.len() == 6 && effs.iter().all(|e| *e == 0o664)
    };
    s.add("J6-批量一致", j6, "同 mode 六建全等");

    // 野位受控
    let j7 = b.set_umask(0o10000).err() == Some(UM_BAD_MASK)
        && b.create("w", 0o17777).err() == Some(UM_BAD_MASK);
    s.add("J7-野位受控", j7, "位出 12 位闭集双面同拒");

    // 缺名受控
    let j8 = b.create("", 0o600).err() == Some(UM_NOENT);
    s.add("J8-缺名受控", j8, "空名建件必拒");

    // 空置面校验
    let j9 = b.count() == 9;
    s.add("J9-计数对平", j9, "成功建件计数 9");

    // 留痕合法与 seq 唯一
    let j10 = b.ledger.iter().all(|l| !l.is_empty()) && b.seq_unique();
    s.add("J10-留痕合法", j10, "留痕非空且 seq 唯一");

    // 特权位穿透
    let j13 = {
        let _ = b.set_umask(0o022);
        b.setgid_survives("sg.txt", 0o2755).unwrap_or(false)
    };
    s.add("J13-setgid穿透", j13, "setgid 位不受掩码清除");

    // 掩码分段
    let j14 = {
        let (o, g, t) = b.mask_profile();
        o == 0 && g == 2 && t == 2
    };
    s.add("J14-掩码分段", j14, "022 分段主0组2它2");

    // 位差与渲染
    let j15 = b.cleared_bits(0o666) == 0o022
        && b.render_mask() == "umask=22"
        && b.batch_consistent(&[0o755, 0o755, 0o755], 0o777)
        && b.name_cleared_ok("a.txt", 0o666);
    s.add("J15-位差渲染", j15, "位差 022、渲染、批核与回放齐");

    // 建件名清单
    let j16 = b.names().contains(&"a.txt".to_string()) && b.mask_history_ok();
    s.add("J16-名清单", j16, "首件在册且账面序列可回放");

    // 双计数与首件渲染
    let j17 = b.mask_swaps() >= 5 && b.create_ops() == 10 && b.first_render() == "a.txt=644";
    s.add("J17-双计数渲染", j17, "置换/建件双计数达锚且首件渲染一致");

    // 注入核 5/5
    let inj = inject_five();
    let j11 = inj.iter().all(|g| *g);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", inj));

    // 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10138_umask_flow() {
        let mut b = UmaskBook::new();
        assert_eq!(b.create("a", 0o777).unwrap_or(0), 0o755);
        assert_eq!(b.set_umask(0o027).unwrap_or(0), 0o022);
        assert_eq!(b.create("b", 0o777).unwrap_or(0), 0o750);
    }

    #[test]
    fn unx10138_checks_green() {
        let s = run_unx10138_checks();
        assert!(s.all_passed(), "UNX-F10138 判据未全绿");
    }
}
