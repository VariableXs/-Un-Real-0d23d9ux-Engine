//! UNX-F10938 ftok/key 冲突错误矩阵（0x1d4c0 段）
//!
//! 纯功能模型：`ftok` 键派生与跨类型 key 冲突裁决。
//! 两错误面：① ftok 引用的文件丢失 → ENOENT；② 同 key 已注册为
//! 另一 IPC 类型时再按新类型取号 → EEXIST 类显式冲突。
//! 键派生按 SVR4 口径：dev 低 8 位 ‖ ino 低 16 位 ‖ proj 低 8 位。
//! 反面千轮样本只走合法 ftok/取号，零误报。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1d4c0;
/// ftok 文件丢失
pub const FK_ENOENT: u32 = SEG_BASE + 0x01;
/// key 冲突且类型不符
pub const FK_ECONFLICT: u32 = SEG_BASE + 0x02;
/// proj 号越界（模型扩展面）
pub const FK_EPROJ: u32 = SEG_BASE + 0x03;
/// 簿内对账失败
pub const FK_LEDGER: u32 = SEG_BASE + 0x04;

// ---------------------------------------------------------------- 常数
/// proj 合法上限（模型 0..=63）
pub const PROJ_MAX: u32 = 63;
/// 簿容量
pub const REG_CAP: usize = 16;
/// 反面验收轮数
pub const FK_ROUNDS: usize = 1000;

// ---------------------------------------------------------------- 数据结构
/// IPC 类型。
#[derive(Clone, Debug, PartialEq)]
pub enum IpcKind {
    /// 消息队列
    MsgQue,
    /// 信号量集合
    SemSet,
    /// 共享内存段
    ShmSeg,
}

impl IpcKind {
    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            IpcKind::MsgQue => "msg",
            IpcKind::SemSet => "sem",
            IpcKind::ShmSeg => "shm",
        }
    }
}

/// 文件节点（dev/ino 对）。
#[derive(Clone, Debug, PartialEq)]
pub struct FileNode {
    /// 路径
    pub path: String,
    /// 设备号
    pub dev: u32,
    /// inode 号
    pub ino: u32,
}

/// 键注册条目。
#[derive(Clone, Debug, PartialEq)]
pub struct KeyReg {
    /// 键值
    pub key: u32,
    /// 注册类型
    pub kind: IpcKind,
    /// 内部句柄
    pub id: u32,
}

/// ftok/取号裁决簿。
#[derive(Clone, Debug)]
pub struct FtokDesk {
    /// 已知文件
    pub files: Vec<FileNode>,
    /// 已注册键
    pub regs: Vec<KeyReg>,
    /// 下一内部句柄
    pub next_id: u32,
    /// 轮次
    pub rounds: usize,
    /// 成功轮
    pub ok_hits: usize,
    /// ENOENT 命中
    pub enoent_hits: usize,
    /// 冲突命中
    pub conflict_hits: usize,
    /// proj 越界命中
    pub eproj_hits: usize,
}

impl FtokDesk {
    /// 建簿：预置若干文件。
    pub fn new(files: Vec<FileNode>) -> Self {
        FtokDesk {
            files,
            regs: Vec::new(),
            next_id: 1,
            rounds: 0,
            ok_hits: 0,
            enoent_hits: 0,
            conflict_hits: 0,
            eproj_hits: 0,
        }
    }

    /// 文件查找。
    pub fn find_file(&self, path: &str) -> Option<usize> {
        let mut i = 0;
        while i < self.files.len() {
            if self.files[i].path == path {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 记账。
    fn book(&mut self, verdict: u32) {
        self.rounds = self.rounds.wrapping_add(1);
        match verdict {
            0 => self.ok_hits = self.ok_hits.wrapping_add(1),
            x if x == FK_ENOENT => self.enoent_hits = self.enoent_hits.wrapping_add(1),
            x if x == FK_ECONFLICT => self.conflict_hits = self.conflict_hits.wrapping_add(1),
            x if x == FK_EPROJ => self.eproj_hits = self.eproj_hits.wrapping_add(1),
            _ => {}
        }
    }

    /// SVR4 口径派生键。
    pub fn derive_key(dev: u32, ino: u32, proj: u32) -> u32 {
        (dev & 0xff)
            | ((ino & 0xffff).wrapping_shl(8))
            | ((proj & 0xff).wrapping_shl(24))
    }

    /// 裁决一次 ftok。判定序：① proj 越界 → EPROJ；
    /// ② 文件查无 → ENOENT；③ 派生并返回键。
    pub fn ftok(&mut self, path: &str, proj: u32) -> Result<u32, u32> {
        if proj > PROJ_MAX {
            self.book(FK_EPROJ);
            return Err(FK_EPROJ);
        }
        match self.find_file(path) {
            None => {
                self.book(FK_ENOENT);
                Err(FK_ENOENT)
            }
            Some(ix) => {
                let key = FtokDesk::derive_key(self.files[ix].dev, self.files[ix].ino, proj);
                self.book(0);
                Ok(key)
            }
        }
    }

    /// 键查找。
    pub fn find_key(&self, key: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.regs.len() {
            if self.regs[i].key == key {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 裁决一次取号。判定序：① 键已注册且类型不同 → ECONFLICT；
    /// ② 键已注册且类型相同 → 命中返回原句柄；③ 新键注册。
    pub fn get(&mut self, key: u32, kind: IpcKind) -> Result<u32, u32> {
        match self.find_key(key) {
            None => {
                if self.regs.len() >= REG_CAP {
                    self.book(FK_LEDGER);
                    return Err(FK_LEDGER);
                }
                let id = self.next_id;
                self.next_id = self.next_id.wrapping_add(1);
                self.regs.push(KeyReg { key, kind, id });
                self.book(0);
                Ok(id)
            }
            Some(ix) => {
                if self.regs[ix].kind != kind {
                    self.book(FK_ECONFLICT);
                    return Err(FK_ECONFLICT);
                }
                let id = self.regs[ix].id;
                self.book(0);
                Ok(id)
            }
        }
    }

    /// 账目对平。
    pub fn ledger_ok(&self) -> bool {
        let total = self
            .ok_hits
            .wrapping_add(self.enoent_hits)
            .wrapping_add(self.conflict_hits)
            .wrapping_add(self.eproj_hits);
        total == self.rounds
    }

    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.files.len() {
            h = (h ^ (self.files[i].dev as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ (self.files[i].ino as u64)).wrapping_mul(0x100000001b3);
            i += 1;
        }
        let mut i = 0;
        while i < self.regs.len() {
            h = (h ^ (self.regs[i].key as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ (self.regs[i].id as u64)).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ (self.rounds as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "FtokDesk{{files={},regs={},rounds={},ok={},enoent={},conflict={},eproj={}}}",
            self.files.len(), self.regs.len(), self.rounds, self.ok_hits,
            self.enoent_hits, self.conflict_hits, self.eproj_hits
        )
    }
}

// ---------------------------------------------------------------- 注入
/// 注入四路坏样本，返回四路是否全部被簿正确拦截。
pub fn inject_four() -> [bool; 4] {
    let mut caught = [false; 4];
    let mut d = FtokDesk::new(vec![
        FileNode { path: "/etc/vx/conf".to_string(), dev: 0x0801, ino: 0x1234 },
        FileNode { path: "/var/vx/run".to_string(), dev: 0x0801, ino: 0x5678 },
    ]);
    // 注入 1：丢失文件 → ENOENT。
    caught[0] = matches!(d.ftok("/no/such/file", 1), Err(x) if x == FK_ENOENT);
    // 注入 2：同键异类型 → ECONFLICT。
    let k1 = d.ftok("/etc/vx/conf", 1).unwrap_or(0);
    let _ = d.get(k1, IpcKind::MsgQue);
    caught[1] = matches!(d.get(k1, IpcKind::SemSet), Err(x) if x == FK_ECONFLICT);
    // 注入 3：proj 越界 → EPROJ。
    caught[2] = matches!(d.ftok("/etc/vx/conf", PROJ_MAX + 1), Err(x) if x == FK_EPROJ);
    // 注入 3 的补路：ENOTDIR 类路径段缺失也归 ENOENT 面。
    caught[2] = caught[2] && matches!(d.ftok("/etc", 1), Err(x) if x == FK_ENOENT || x == FK_EPROJ || x == 0);
    // 注入 4：同键同类型幂等命中（非错误）+ 派生键不同 proj 互异。
    let id_same = d.get(k1, IpcKind::MsgQue);
    let k2 = d.ftok("/etc/vx/conf", 2).unwrap_or(0);
    caught[3] = matches!(id_same, Ok(_))
        && k2 != k1
        && matches!(d.get(k2, IpcKind::ShmSeg), Ok(_));
    caught
}

// ---------------------------------------------------------------- 确定性样本
/// 千轮样本：合法 ftok/取号占七成，三错误面轮转。返回符合预期轮数。
pub fn sample_thousand(d: &mut FtokDesk) -> usize {
    let mut good = 0;
    let mut r = 0;
    while r < FK_ROUNDS {
        let low = r % 10;
        let res: Result<u32, u32>;
        let expect_ok;
        if low < 4 {
            // 合法 ftok：轮转文件与 proj。
            res = d.ftok(
                if r % 2 == 0 { "/etc/vx/conf" } else { "/var/vx/run" },
                (r % 8) as u32,
            );
            expect_ok = true;
        } else if low < 7 {
            // 合法取号：对既有键同类型幂等命中。
            let key = FtokDesk::derive_key(0x0801, 0x1234, (r % 8) as u32);
            res = d.get(key, IpcKind::SemSet);
            expect_ok = true;
        } else if low == 7 {
            // ENOENT 面。
            res = d.ftok("/ghost/path", (r % 4) as u32);
            expect_ok = false;
        } else if low == 8 {
            // ECONFLICT 面：先占 msg 再以 shm 取同键。
            let key = FtokDesk::derive_key(0x0801, 0x5678, (r % 4) as u32);
            let first = d.get(key, IpcKind::MsgQue);
            let second = d.get(key, IpcKind::ShmSeg);
            res = match first {
                Ok(_) => second,
                Err(e) => Err(e),
            };
            expect_ok = false;
        } else {
            // EPROJ 面。
            res = d.ftok("/etc/vx/conf", PROJ_MAX + 1 + (r % 5) as u32);
            expect_ok = false;
        }
        let matched = match &res {
            Ok(_) => expect_ok,
            Err(x) => {
                !expect_ok && (*x == FK_ENOENT || *x == FK_ECONFLICT || *x == FK_EPROJ)
            }
        };
        if matched {
            good += 1;
        }
        r += 1;
    }
    good
}

// ---------------------------------------------------------------- 判据
/// UNX-F10938 判据组。
pub fn run_unx10938_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10938");
    let mk = || {
        FtokDesk::new(vec![
            FileNode { path: "/etc/vx/conf".to_string(), dev: 0x0801, ino: 0x1234 },
            FileNode { path: "/var/vx/run".to_string(), dev: 0x0801, ino: 0x5678 },
        ])
    };
    // J1 正面之一：ftok 文件丢失 → ENOENT 十轮全中。
    let mut d1 = mk();
    let mut enoent_cnt = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(d1.ftok("/ghost/x", k as u32), Err(x) if x == FK_ENOENT) {
            enoent_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面ENOENT", enoent_cnt == 10, "ftok 文件丢失十轮全部裁 ENOENT");
    // J1 正面之二：key 冲突类型不符 → ECONFLICT 十轮全中。
    let mut d2 = mk();
    let k2 = d2.ftok("/etc/vx/conf", 1).unwrap_or(0);
    let _ = d2.get(k2, IpcKind::MsgQue);
    let mut conflict_cnt = 0;
    let mut k = 0;
    while k < 10 {
        let other = if k % 2 == 0 { IpcKind::SemSet } else { IpcKind::ShmSeg };
        if matches!(d2.get(k2, other), Err(x) if x == FK_ECONFLICT) {
            conflict_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面冲突", conflict_cnt == 10, "同键异类型十轮全部显式裁冲突");
    // J2 反面：千轮合法样本零误报。
    let mut d3 = mk();
    let good = sample_thousand(&mut d3);
    s.add_dyn("J2-反面零误报", good == FK_ROUNDS, &format!("千轮样本符合预期 {} 轮", good));
    // J3 注入四路全捕获。
    let inj = inject_four();
    s.add(
        "J3-注入四路",
        inj[0] && inj[1] && inj[2] && inj[3],
        "ENOENT/冲突/EPROJ/幂等四路样本全部按预期",
    );
    // J4 派生键口径：SVR4 位布局逐位核验。
    let kv = FtokDesk::derive_key(0x12_34_56_78, 0xAB_CD_EF01, 0x2A);
    s.add(
        "J4-派生口径",
        kv == 0x2aef0178,
        "dev 低 8 位 ‖ ino 低 16 位 ‖ proj 低 8 位布局正确",
    );
    // J5 同文件不同 proj 键互异（0..8 全域互异）。
    let mut keys: Vec<u32> = Vec::new();
    let mut k = 0;
    while k < 8 {
        keys.push(FtokDesk::derive_key(0x0801, 0x1234, k as u32));
        k += 1;
    }
    let mut all_diff = true;
    let mut a = 0;
    while a < keys.len() {
        let mut b = a + 1;
        while b < keys.len() {
            if keys[a] == keys[b] {
                all_diff = false;
            }
            b += 1;
        }
        a += 1;
    }
    s.add("J5-proj互异", all_diff, "同文件 0..8 号 proj 派生键两两不同");
    // J6 错误码分段。
    s.add(
        "J6-错误码分段",
        FK_ENOENT > SEG_BASE && FK_LEDGER < SEG_BASE + 0x20,
        "四个错误码全部落在段基址页内",
    );
    // J7 错误码互异。
    let codes = [FK_ENOENT, FK_ECONFLICT, FK_EPROJ, FK_LEDGER];
    let mut distinct = true;
    let mut a = 0;
    while a < codes.len() {
        let mut b = a + 1;
        while b < codes.len() {
            if codes[a] == codes[b] {
                distinct = false;
            }
            b += 1;
        }
        a += 1;
    }
    s.add("J7-错误码互异", distinct, "四个错误码两两不同");
    // J8 账目对平。
    s.add("J8-账目对平", d3.ledger_ok(), "千轮之后四类计数与轮次对平");
    // J9 fold 稳定。
    let f = d3.fold();
    let clone = d3.clone();
    s.add("J9-fold稳定", clone.fold() == f, "克隆后指纹逐位一致");
    // J10 同键同类型幂等。
    let mut d4 = mk();
    let k4 = d4.ftok("/var/vx/run", 3).unwrap_or(0);
    let i1 = d4.get(k4, IpcKind::ShmSeg);
    let i2 = d4.get(k4, IpcKind::ShmSeg);
    s.add("J10-幂等命中", i1.is_ok() && i1 == i2, "同键同类型两次取号返回同句柄");
    // J11 渲染非空。
    let txt = d3.render();
    s.add("J11-渲染非空", txt.len() > 10 && txt.contains("FtokDesk"), "簿级渲染含类型名与计数");
    // J12 proj 越界面。
    let r12 = d4.ftok("/etc/vx/conf", PROJ_MAX + 1);
    s.add("J12-EPROJ面", matches!(r12, Err(x) if x == FK_EPROJ), "proj 越界裁 EPROJ");
    // J13 动态：千轮明细。
    s.add_dyn(
        "J13-千轮明细",
        good == FK_ROUNDS,
        &format!("good={} desk={}", good, d3.render()),
    );
    // J14 动态：注入明细。
    s.add_dyn(
        "J14-注入明细",
        inj.iter().all(|x| *x),
        &format!("inj={:?}", inj),
    );
    s
}

// ---------------------------------------------------------------- 测试
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10938_ftok_conflict_flow() {
        // 流程：合法 ftok → 注册 msg → 冲突 → 丢失文件 → proj 越界。
        let mut d = FtokDesk::new(vec![
            FileNode { path: "/a".to_string(), dev: 0x0801, ino: 0x1234 },
            FileNode { path: "/b".to_string(), dev: 0x0801, ino: 0x5678 },
        ]);
        let k = d.ftok("/a", 1).unwrap_or(0);
        assert_eq!(k, 0x01123401);
        let id = d.get(k, IpcKind::MsgQue);
        assert!(id.is_ok());
        assert!(matches!(d.get(k, IpcKind::ShmSeg), Err(x) if x == FK_ECONFLICT));
        assert!(matches!(d.ftok("/zz", 1), Err(x) if x == FK_ENOENT));
        assert!(matches!(d.ftok("/a", 99), Err(x) if x == FK_EPROJ));
        assert!(d.ledger_ok());
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx10938_checks_green() {
        let s = run_unx10938_checks();
        assert!(s.all_passed(), "UNX-F10938 判据未全绿");
    }
}
