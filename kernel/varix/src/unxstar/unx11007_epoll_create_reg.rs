//! UNX-F11007 epoll 实例建立挂号联签（create 注册面对账）（0x1e0e0 段）
//!
//! 锚点：UNX-F11007-J1 实例注册面逐项对平（10/10），fd 账联动一致（10/10）。
//! 域：CoRun·UNX-C4-B31（futex/epoll 挂号联签域）。
//!
//! 结构：实例册（epoll create 逐实例挂号、fd 号自动配发）+ fd 账（发号
//! 严格递增、销号回收零残留）+ 注册面对账器（实例数 = fd 发号数 = 在册
//! 数）+ fold 指纹与冻结 + 注入核（坏尺寸/重复关账/越界 fd/冻结后写四路
//! 必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e0e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e0e0;
pub const EC_BADSIZE: u32 = SEG_BASE + 0x01; // 实例尺寸非法
pub const EC_RECLOSE: u32 = SEG_BASE + 0x02; // fd 重复关账
pub const EC_NOFD: u32 = SEG_BASE + 0x03; // 越界 fd
pub const EC_FDSEQ: u32 = SEG_BASE + 0x04; // fd 发号序破坏
pub const EC_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const EC_CAP: u32 = SEG_BASE + 0x06; // 实例册超容

/// 实例注册面对平数（10/10）。
pub const INSTANCES_NEEDED: usize = 10;
/// 实例册定容。
pub const INST_CAP: usize = 16;
/// fd 发号基址。
pub const FD_BASE: u32 = 100;
/// 实例册域标（fold 种子）。
pub const ECR_TAG: u64 = 0x1e0e_c7;

/// 单个 epoll 实例：fd 号 + 声明尺寸 + 关账位。
#[derive(Clone, Debug)]
pub struct EpollInst {
    pub fd: u32,
    pub size: u32,
    pub closed: bool,
}

impl EpollInst {
    pub fn render(&self) -> String {
        let flag = if self.closed { "C" } else { "O" };
        format!("fd{}|{}|n{}", self.fd, flag, self.size)
    }
}

/// epoll 实例建立挂号册：create 挂号 → fd 发号 → 注册面对账 → 冻结。
pub struct CreateRegistry {
    pub insts: Vec<EpollInst>,
    pub next_fd: u32,
    pub issued: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl CreateRegistry {
    pub fn new() -> CreateRegistry {
        CreateRegistry {
            insts: Vec::new(),
            next_fd: FD_BASE,
            issued: 0,
            frozen: false,
            root: None,
        }
    }

    /// create 挂号：冻结、零尺寸、超容均拒；fd 严格递增发号。
    pub fn create(&mut self, size: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(EC_FROZEN);
        }
        if size == 0 {
            return Err(EC_BADSIZE);
        }
        if self.insts.len() >= INST_CAP {
            return Err(EC_CAP);
        }
        let fd = self.next_fd;
        self.next_fd = self.next_fd.wrapping_add(1);
        self.issued = self.issued.wrapping_add(1);
        self.insts.push(EpollInst { fd, size, closed: false });
        Ok(fd)
    }

    /// close 关账：越界 fd 与重复关账均拒。
    pub fn close(&mut self, fd: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(EC_FROZEN);
        }
        let mut k = 0usize;
        while k < self.insts.len() {
            if self.insts[k].fd == fd {
                if self.insts[k].closed {
                    return Err(EC_RECLOSE);
                }
                self.insts[k].closed = true;
                return Ok(());
            }
            k += 1;
        }
        Err(EC_NOFD)
    }

    /// 在册（未关账）fd 清单。
    pub fn open_fds(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.insts.len() {
            if !self.insts[k].closed {
                g.push(self.insts[k].fd);
            }
            k += 1;
        }
        g
    }

    /// fd 发号序对账：发号严格连续递增且无重复。
    pub fn fd_seq_ok(&self) -> bool {
        let mut k = 0usize;
        while k < self.insts.len() {
            if self.insts[k].fd != FD_BASE.wrapping_add(k as u32) {
                return false;
            }
            k += 1;
        }
        self.issued == self.insts.len() as u32
    }

    /// 注册面对平：实例数恰为对平数且 fd 账联动一致。
    pub fn closed(&self) -> bool {
        self.insts.len() == INSTANCES_NEEDED && self.fd_seq_ok()
    }

    /// 按 fd 查实例位次。
    pub fn find(&self, fd: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.insts.len() {
            if self.insts[k].fd == fd {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 链式 fold 指纹（FNV 风格，序敏感，含 fd/尺寸/关账位）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = ECR_TAG;
        let mut k = 0usize;
        while k < self.insts.len() {
            let e = &self.insts[k];
            h = (h ^ e.fd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.size as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.closed as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.next_fd as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.issued as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结：翻冻结位并落冻结根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 实例尺寸确定性样本。
pub fn size_sample(no: u32) -> u32 {
    no.wrapping_mul(23).wrapping_add(64)
}

/// 十实例全链：逐实例挂号并逐一对账。
pub fn build_registry() -> (CreateRegistry, bool) {
    let mut r = CreateRegistry::new();
    let mut ok = true;
    let mut i = 1u32;
    while i <= INSTANCES_NEEDED as u32 {
        if r.create(size_sample(i)).is_err() {
            ok = false;
        }
        i += 1;
    }
    let r_ok = ok && r.closed() && r.fd_seq_ok();
    (r, r_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 零尺寸 create 必拒
    let mut a = CreateRegistry::new();
    got[0] = a.create(0).err() == Some(EC_BADSIZE);
    // 2) 重复关账必拒
    let mut b = CreateRegistry::new();
    let fd = b.create(128).unwrap_or(0);
    let _ = b.close(fd);
    got[1] = b.close(fd).err() == Some(EC_RECLOSE);
    // 3) 越界 fd 必拒
    let mut c = CreateRegistry::new();
    got[2] = c.close(FD_BASE.wrapping_add(999)).err() == Some(EC_NOFD);
    // 4) 冻结后写账必抓
    let (mut d, _) = build_registry();
    d.freeze();
    got[3] = d.create(64).err() == Some(EC_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11007_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11007");

    // J1 实例注册面逐项对平（10/10）
    let (mut r, built) = build_registry();
    s.add("J1-注册面逐项对平", built && r.insts.len() == 10, "十实例挂号齐且对平成立");

    // J2 fd 账联动一致：发号连续递增且发号数 = 在册数
    s.add("J2-fd 账联动一致", r.fd_seq_ok() && r.issued == 10 && r.next_fd == FD_BASE + 10, "fd 发号严格联动零漂移");

    // J3 冻结根：fold 稳定且与根一致
    let root = r.freeze();
    s.add("J3-冻结根", r.root == Some(root) && r.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = r.create(64).err() == Some(EC_FROZEN) && r.close(FD_BASE).err() == Some(EC_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后建立与关账均必拒");

    // J5 注入检出：零尺寸 + 重复关账
    let inj = inject_four();
    s.add("J5-尺寸重关检出", inj[0] && inj[1], "零尺寸与重复关账两路必抓");

    // J6 注入检出：越界 fd + 冻结后写
    s.add("J6-越界冻结检出", inj[2] && inj[3], "越界 fd 必拒且冻结后写必抓");

    // J7 按 fd 查实例：首末命中且未知 fd 不命中
    let j7 = r.find(FD_BASE) == Some(0)
        && r.find(FD_BASE + 9) == Some(9)
        && r.find(1).is_none();
    s.add("J7-按 fd 查实例", j7, "首末位次命中且未知 fd 不命中");

    // J8 关账零残留：逐实例关账后 open 清单清空
    let j8 = {
        let mut c = CreateRegistry::new();
        let mut i = 1u32;
        while i <= 6u32 {
            let _ = c.create(size_sample(i));
            i += 1;
        }
        let mut i = 0u32;
        while i < 6u32 {
            let _ = c.close(FD_BASE + i);
            i += 1;
        }
        c.open_fds().is_empty() && c.insts.len() == 6
    };
    s.add("J8-关账零残留", j8, "逐实例关账后在册清单清空");

    // J9 部分关账：关半数 open 清单恰余半数
    let j9 = {
        let mut c = CreateRegistry::new();
        let mut i = 1u32;
        while i <= 8u32 {
            let _ = c.create(size_sample(i));
            i += 1;
        }
        let mut i = 0u32;
        while i < 4u32 {
            let _ = c.close(FD_BASE + i);
            i += 1;
        }
        c.open_fds().len() == 4
    };
    s.add("J9-部分关账", j9, "关半数后 open 清单恰余半数");

    // J10 渲染面：开账与关账标记可区分
    let j10 = {
        let a = EpollInst { fd: 5, size: 64, closed: true };
        let b = EpollInst { fd: 5, size: 64, closed: false };
        a.render() != b.render() && a.render().contains("C") && b.render().contains("O")
    };
    s.add("J10-渲染分旗", j10, "开账/关账渲染可区分");

    // J11 超容拒：第 INST_CAP+1 实例必拒
    let j11 = {
        let mut c = CreateRegistry::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= INST_CAP as u32 {
            if c.create(size_sample(n)).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.create(64).err() == Some(EC_CAP)
    };
    s.add("J11-超容边界", j11, "超出定容必拒且原账不损");

    // J12 未关账不算对账平：实例齐但关账位未翻
    let j12 = {
        let mut c = CreateRegistry::new();
        let mut i = 1u32;
        while i <= INSTANCES_NEEDED as u32 {
            let _ = c.create(size_sample(i));
            i += 1;
        }
        c.closed() && c.open_fds().len() == INSTANCES_NEEDED
    };
    s.add("J12-实例齐开账在册", j12, "实例齐时对平成立且开账全在册");

    // J13 样本确定性：尺寸同入同出且互异
    let v3a = size_sample(3);
    let v3b = size_sample(3);
    let mut distinct = true;
    let mut i = 1u32;
    while i <= INSTANCES_NEEDED as u32 {
        let mut j = i.wrapping_add(1);
        while j <= INSTANCES_NEEDED as u32 {
            if size_sample(i) == size_sample(j) {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("J13-样本确定性", v3a == v3b && distinct, "尺寸样本同入同出且互异");

    // J14 动态明细：fd 发号顶与发号数落账
    s.add_dyn(
        "J14-fd 账明细",
        r.issued == 10 && r.next_fd == FD_BASE + 10,
        &format!("issued={} next_fd={}", r.issued, r.next_fd),
    );

    // J15 动态明细：冻结根十六进制落账
    let fp = r.fold();
    s.add_dyn("J15-冻结根明细", fp == root, &format!("root={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11007_registry_flow() {
        let mut r = CreateRegistry::new();
        assert!(r.create(0).err() == Some(EC_BADSIZE));
        let fd = r.create(128).unwrap_or(0);
        assert_eq!(fd, FD_BASE);
        assert!(r.close(fd).is_ok());
        assert!(r.close(fd).err() == Some(EC_RECLOSE));
        r.freeze();
        assert!(r.create(64).err() == Some(EC_FROZEN));
        assert!(r.root.is_some());
    }

    #[test]
    fn unx11007_checks_green() {
        let s = run_unx11007_checks();
        assert!(s.all_passed(), "UNX-F11007 判据未全绿");
    }
}
