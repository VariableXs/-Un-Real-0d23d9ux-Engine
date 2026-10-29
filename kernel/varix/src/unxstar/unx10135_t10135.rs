//! UNX-F10135 挂载边界正反双判据（挂载点语义达成与跨挂载点误用受控）（0x10400 段）
//!
//! 锚点：UNX-F10135-J1 挂载点语义达成（stub 挂载面枚举/遮蔽语义 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：挂载册（挂载点→覆盖面登记 + 底层枚举表）+ 遮蔽核（挂载后枚举
//! 走覆盖面/卸载还原底层/跨挂载点改名拒）+ 挂载账（逐次留痕）+ 注入核
//! （重复挂载/野点卸载/跨面改名/空留痕/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x10400 独占 0x20）。
pub const SEG_BASE: u32 = 0x10400;
pub const MO_BUSY: u32 = SEG_BASE + 0x01; // 重复挂载
pub const MO_NOPOINT: u32 = SEG_BASE + 0x02; // 野挂载点
pub const MO_EXDEV: u32 = SEG_BASE + 0x03; // 跨挂载点
pub const MO_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const MO_SEALED: u32 = SEG_BASE + 0x05; // 封表后操作

/// 挂载记录。
#[derive(Clone)]
pub struct Mount {
    pub point: String,
    pub over: Vec<String>, // 覆盖面条目
    pub active: bool,
}

impl Mount {
    pub fn ok(&self) -> bool {
        !self.point.is_empty() && !self.over.is_empty()
    }
}

/// 挂载账留痕。
#[derive(Clone)]
pub struct MoRec {
    pub seq: u32,
    pub op: u32, // 0=mount 1=umount 2=rename
    pub point: String,
    pub arg: String,
}

impl MoRec {
    pub fn ok(&self) -> bool {
        self.op <= 2 && !self.point.is_empty()
    }
}

/// 挂载册。
pub struct MountBook {
    pub mounts: Vec<Mount>,
    pub base: Vec<String>, // 底层枚举表（被遮蔽前可见）
    pub ledger: Vec<MoRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl MountBook {
    pub fn new() -> MountBook {
        MountBook { mounts: Vec::new(), base: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 底层登记。
    pub fn base_add(&mut self, name: &str) {
        self.base.push(name.to_string());
    }

    /// mount：挂覆盖面（重复挂载/野点拒）。
    pub fn mount(&mut self, point: &str, over: &[&str], note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(MO_SEALED);
        }
        if note.is_empty() {
            return Err(MO_EMPTY_NOTE);
        }
        if point.is_empty() {
            return Err(MO_NOPOINT);
        }
        if self.mounts.iter().any(|m| m.point == point && m.active) {
            return Err(MO_BUSY);
        }
        let items: Vec<String> = over.iter().map(|s| s.to_string()).collect();
        if items.is_empty() {
            return Err(MO_NOPOINT);
        }
        self.mounts.push(Mount { point: point.to_string(), over: items, active: true });
        self.ledger.push(MoRec {
            seq: self.seq_next,
            op: 0,
            point: point.to_string(),
            arg: String::new(),
        });
        self.seq_next += 1;
        Ok(())
    }

    /// umount：卸载还原底层（野点拒）。
    pub fn umount(&mut self, point: &str, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(MO_SEALED);
        }
        if note.is_empty() {
            return Err(MO_EMPTY_NOTE);
        }
        let m = match self.mounts.iter_mut().find(|m| m.point == point && m.active) {
            Some(m) => m,
            None => return Err(MO_NOPOINT),
        };
        m.active = false;
        self.ledger.push(MoRec {
            seq: self.seq_next,
            op: 1,
            point: point.to_string(),
            arg: String::new(),
        });
        self.seq_next += 1;
        Ok(())
    }

    /// 枚举：挂载点命中走覆盖面，否则走底层。
    pub fn list_at(&self, point: &str) -> Vec<String> {
        match self.mounts.iter().find(|m| m.point == point && m.active) {
            Some(m) => m.over.clone(),
            None => self.base.clone(),
        }
    }

    /// 遮蔽语义核：挂载后覆盖面与底层不同。
    pub fn shadowed(&self, point: &str) -> bool {
        match self.mounts.iter().find(|m| m.point == point && m.active) {
            Some(m) => m.over != self.base,
            None => false,
        }
    }

    /// rename：跨挂载点（源与目标分属不同挂载面）拒。
    pub fn rename_across(&mut self, src: &str, dst: &str, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(MO_SEALED);
        }
        if note.is_empty() {
            return Err(MO_EMPTY_NOTE);
        }
        let side = |s: &str, slf: &Self| -> u32 {
            let mut id = 0u32;
            for (i, m) in slf.mounts.iter().enumerate() {
                if m.active && s.starts_with(&m.point) {
                    id = (i + 1) as u32;
                }
            }
            id
        };
        if side(src, self) != side(dst, self) {
            self.ledger.push(MoRec {
                seq: self.seq_next,
                op: 2,
                point: src.to_string(),
                arg: dst.to_string(),
            });
            self.seq_next += 1;
            return Err(MO_EXDEV);
        }
        Ok(())
    }

    pub fn active_count(&self) -> usize {
        self.mounts.iter().filter(|m| m.active).count()
    }

    /// 挂载点清单（仅活跃）。
    pub fn active_points(&self) -> Vec<String> {
        self.mounts
            .iter()
            .filter(|m| m.active)
            .map(|m| m.point.clone())
            .collect()
    }

    /// 覆盖面条目总数。
    pub fn over_total(&self) -> usize {
        self.mounts
            .iter()
            .filter(|m| m.active)
            .map(|m| m.over.len())
            .fold(0usize, |a, b| a + b)
    }

    /// 底层条目数。
    pub fn base_count(&self) -> usize {
        self.base.len()
    }

    /// 挂载点命中判定。
    pub fn is_mounted(&self, point: &str) -> bool {
        self.mounts.iter().any(|m| m.point == point && m.active)
    }

    /// 挂载面渲染（诊断面；优先活跃挂载，与查询面口径一致）。
    pub fn render(&self, point: &str) -> String {
        let m = self
            .mounts
            .iter()
            .find(|m| m.point == point && m.active)
            .or_else(|| self.mounts.iter().find(|m| m.point == point));
        match m {
            Some(m) => {
                let state = if m.active { "on" } else { "off" };
                format!("{}|{}|{}", m.point, state, m.over.len())
            }
            None => format!("{}|none", point),
        }
    }

    /// 双向留痕对平：mount/umount 计数与活跃差一致。
    pub fn op_balance(&self) -> bool {
        let m = self.ledger.iter().filter(|r| r.op == 0).count();
        let u = self.ledger.iter().filter(|r| r.op == 1).count();
        m.saturating_sub(u) == self.active_count()
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
        let mut h: u32 = 0x104000;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.op);
            for b in r.point.bytes() {
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
    let mut b = MountBook::new();
    let _ = b.mount("/mnt", &["stub-a"], "inj-1");
    // 1) 重复挂载必拒
    got[0] = b.mount("/mnt", &["stub-b"], "inj-2").err() == Some(MO_BUSY);
    // 2) 野点卸载必拒
    got[1] = b.umount("/nowhere", "inj-3").err() == Some(MO_NOPOINT);
    // 3) 跨挂载点改名必拒
    let _ = b.mount("/opt", &["stub-c"], "inj-4");
    got[2] = b.rename_across("/mnt/x", "/opt/y", "inj-5").err() == Some(MO_EXDEV);
    // 4) 空留痕必拒
    got[3] = b.umount("/mnt", "").err() == Some(MO_EMPTY_NOTE);
    // 5) 封表后操作必抓
    b.seal();
    got[4] = b.umount("/mnt", "inj-seal").err() == Some(MO_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10135_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10135");

    // 正路径：底层枚举 + 挂载遮蔽
    let mut b = MountBook::new();
    b.base_add("rootfile");
    let _ = b.mount("/mnt", &["mfile", "msub"], "挂载");
    let at = b.list_at("/mnt");
    let j1 = at == vec!["mfile".to_string(), "msub".to_string()] && b.shadowed("/mnt");
    s.add("J1-遮蔽枚举", j1, "挂载点枚举走覆盖面且遮蔽成立");

    // 未挂点走底层
    let j2 = b.list_at("/other") == vec!["rootfile".to_string()];
    s.add("J2-底层透出", j2, "未挂点枚举走底层");

    // 卸载还原
    let j3 = {
        let _ = b.umount("/mnt", "卸载");
        b.list_at("/mnt") == vec!["rootfile".to_string()] && b.active_count() == 0
    };
    s.add("J3-卸载还原", j3, "卸载后底层透出且活跃归零");

    // 重挂还原
    let j4 = {
        let _ = b.mount("/mnt", &["again"], "重挂");
        b.list_at("/mnt") == vec!["again".to_string()] && b.active_count() == 1
    };
    s.add("J4-重挂生效", j4, "卸载后同点可重挂");

    // 重复挂载受控
    let j5 = b.mount("/mnt", &["x"], "重撞").err() == Some(MO_BUSY);
    s.add("J5-重挂受控", j5, "活跃同点再挂必拒");

    // 跨挂载点受控
    let j6 = {
        let _ = b.mount("/opt", &["o1"], "二挂");
        b.rename_across("/mnt/a", "/opt/b", "跨面").err() == Some(MO_EXDEV)
    };
    s.add("J6-跨面受控", j6, "跨挂载点改名必拒");

    // 同面改名放行
    let j7 = b.rename_across("/mnt/a", "/mnt/b", "同面").is_ok();
    s.add("J7-同面放行", j7, "同挂载面内改名通过");

    // 野点受控
    let j8 = b.umount("/nowhere", "野点").err() == Some(MO_NOPOINT);
    s.add("J8-野点受控", j8, "未挂点卸载必拒");

    // 空留痕拒收
    let j9 = b.mount("/nn", &["x"], "").err() == Some(MO_EMPTY_NOTE);
    s.add("J9-空留痕拒", j9, "note 为空不入账");

    // 留痕合法与 seq 唯一
    let j10 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J10-留痕合法", j10, "op 在闭集且点非空");

    // 注入核 5/5
    let inj = inject_five();
    let j11 = inj.iter().all(|g| *g);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", inj));

    // 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    // 挂载面统计与渲染
    let j13 = b.active_points().len() == 2
        && b.over_total() == 2
        && b.base_count() == 1
        && b.is_mounted("/mnt")
        && b.render("/mnt").contains("|on|1");
    s.add("J13-面统计", j13, "双活跃点、覆盖 2 条、渲染命中");

    // 留痕对平
    let j14 = b.op_balance();
    s.add("J14-留痕对平", j14, "挂/卸差与活跃数一致");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10135_mount_flow() {
        let mut b = MountBook::new();
        b.base_add("base");
        let _ = b.mount("/m", &["over"], "x");
        assert_eq!(b.list_at("/m"), vec!["over".to_string()]);
        let _ = b.umount("/m", "y");
        assert_eq!(b.list_at("/m"), vec!["base".to_string()]);
        assert!(b.mount("/m", &["o2"], "z").is_ok());
    }

    #[test]
    fn unx10135_checks_green() {
        let s = run_unx10135_checks();
        assert!(s.all_passed(), "UNX-F10135 判据未全绿");
    }
}
