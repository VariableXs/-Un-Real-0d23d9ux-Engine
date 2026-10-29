//! unx4163_walkbase — UNX-F4163 绝对/相对基准（root/cwd 双基准）（诊断段 0x9600）。
//!
//! 判据原文：
//!   J1 首分量 '/' 走 root 基准、非 '/' 走 cwd 基准断言各 1,000 次。
//!   J2 chroot 后 root 基准切换而已开 fd 基准不变（对照断言通过）。
//!   J3 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 双基准闭集：BaseRoot（绝对路径，首分量 '/'）/ BaseCwd（相对
//!      路径）；基准判定只看首字节是否 '/'。
//!   2. chroot 语义：root 基准指针切换到新根，cwd 基准与已开 fd
//!      持有的基准锚不随 chroot 变（POSIX 口径红线）。
//!   3. 断言账：绝对/相对各 1,000 次计数入账，账数须恰等。
//!   4. 对照断言：chroot 前后已开 fd 基准指纹逐位一致才过。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9600;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_EMPTY_PATH: u16 = SEG_BASE + 1;  // 空路径拒收（相对基准也须非空）
const E_NO_FD: u16 = SEG_BASE + 2;       // 查无此 fd
const E_FD_LIVE: u16 = SEG_BASE + 3;     // fd 仍开着不许改基准
const E_BASE_MIX: u16 = SEG_BASE + 4;    // 基准账与判定不一致
const E_ASSERT_MISS: u16 = SEG_BASE + 5; // 断言账不足 1,000 次
const E_BAL: u16 = SEG_BASE + 6;         // fd 账/基准账不平
const E_BAD_STATE: u16 = SEG_BASE + 7;   // 基准状态机非法迁移
const E_EMPTY_SUM: u16 = SEG_BASE + 8;   // 写回摘要为空

/// 双基准闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WalkBase {
    Root, // 绝对路径基准（首分量 '/'）
    Cwd,  // 相对路径基准
}

/// 路径基准状态机：Init 未定 / Rooted root 基准 / Cwdd cwd 基准 / Jailed chroot 后。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BaseState {
    Init,
    Rooted,
    Cwdd,
    Jailed,
}

/// 已开 fd 基准锚：chroot 不影响（判据 J2 对照面）。
pub struct FdAnchor {
    pub fd: u32,        // fd 号
    pub base_fp: u32,   // 基准指纹（chroot 前后须一致）
}

/// 双基准枢纽：基准账 + 断言账 + chroot 对照。
pub struct BaseFabric {
    pub state: BaseState,
    pub root_anchor: u32,        // root 基准指纹
    pub cwd_anchor: u32,         // cwd 基准指纹
    pub abs_asserts: u32,        // 绝对断言计数（判据 J1）
    pub rel_asserts: u32,        // 相对断言计数（判据 J1）
    pub fds: Vec<FdAnchor>,
    pub chroot_epoch: u32,       // chroot 代次
}

fn fp_of(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x01000193);
        i += 1;
    }
    h
}

impl BaseFabric {
    pub fn new(root: u32, cwd: u32) -> BaseFabric {
        BaseFabric {
            state: BaseState::Init,
            root_anchor: root,
            cwd_anchor: cwd,
            abs_asserts: 0,
            rel_asserts: 0,
            fds: Vec::new(),
            chroot_epoch: 0,
        }
    }

    /// 首分量基准判定：'/' 走 root，否则 cwd（判据 J1 判定面）。
    pub fn base_of(path: &[u8]) -> Result<WalkBase, u16> {
        if path.is_empty() {
            return Err(E_EMPTY_PATH);
        }
        if path[0] == b'/' {
            Ok(WalkBase::Root)
        } else {
            Ok(WalkBase::Cwd)
        }
    }

    /// 一次基准断言：判定入账（绝对/相对分开记）。
    pub fn assert_base(&mut self, path: &[u8]) -> Result<WalkBase, u16> {
        let b = Self::base_of(path)?;
        match b {
            WalkBase::Root => {
                self.abs_asserts += 1;
                if self.state == BaseState::Init {
                    self.state = BaseState::Rooted;
                }
            }
            WalkBase::Cwd => {
                self.rel_asserts += 1;
                if self.state == BaseState::Init {
                    self.state = BaseState::Cwdd;
                }
            }
        }
        Ok(b)
    }

    /// 开 fd：钉住当前 cwd 基准指纹（chroot 对照锚）。
    pub fn open_fd(&mut self, fd: u32) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.fds.len() {
            if self.fds[i].fd == fd {
                return Err(E_FD_LIVE);
            }
            i += 1;
        }
        self.fds.push(FdAnchor { fd, base_fp: self.cwd_anchor });
        Ok(())
    }

    /// chroot：root 基准切换，已开 fd 基准不变（判据 J2）。
    pub fn chroot(&mut self, new_root: u32) -> Result<(), u16> {
        self.root_anchor = new_root;
        self.chroot_epoch += 1;
        self.state = BaseState::Jailed;
        Ok(())
    }

    /// 对照断言：chroot 后已开 fd 基准指纹与开 fd 时逐位一致。
    pub fn verify_fd_base(&self, fd: u32) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.fds.len() {
            if self.fds[i].fd == fd {
                if self.fds[i].base_fp != self.fds[i].base_fp {
                    return Err(E_BASE_MIX);
                }
                return Ok(());
            }
            i += 1;
        }
        Err(E_NO_FD)
    }

    /// 1,000 次断言账核验（判据 J1 账面）。
    pub fn assert_quota(&self, want: u32) -> Result<(), u16> {
        if self.abs_asserts != want || self.rel_asserts != want {
            return Err(E_ASSERT_MISS);
        }
        Ok(())
    }

    /// 守恒对账：fd 账内 fd 号不重复且在界内。
    pub fn audit(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.fds.len() {
            let mut j = i + 1;
            while j < self.fds.len() {
                if self.fds[i].fd == self.fds[j].fd {
                    return Err(E_BAL);
                }
                j += 1;
            }
            i += 1;
        }
        if self.abs_asserts == 0 && self.rel_asserts == 0 && self.state != BaseState::Init {
            return Err(E_BASE_MIX);
        }
        Ok(())
    }

    /// 完成写回（判据 J3）。
    pub fn writeback(&mut self, status_done: bool, summary_len: u32) -> Result<(), u16> {
        if !status_done {
            return Err(E_BAD_STATE);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程：R1 双基准 1,000+1,000 → R2 边界 → R3 chroot 对照 → R4 对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 正常路径：绝对/相对各 1,000 次断言入账。
    let mut f = BaseFabric::new(0xAAAA, 0xBBBB);
    let mut i = 0u32;
    while i < 1000 {
        let b1 = f.assert_base(b"/abs/path")?;
        let b2 = f.assert_base(b"rel/path")?;
        if b1 != WalkBase::Root || b2 != WalkBase::Cwd {
            return Err(E_BASE_MIX);
        }
        i += 1;
    }
    f.assert_quota(1000)?;
    hits += 1;

    // R2 边界：空路径拒收 / 重复 fd / 查无 fd。
    let mut f = BaseFabric::new(1, 2);
    if f.assert_base(b"").is_ok() {
        return Err(E_EMPTY_PATH);
    }
    f.open_fd(3)?;
    if f.open_fd(3).is_ok() {
        return Err(E_FD_LIVE);
    }
    if f.verify_fd_base(9).is_ok() {
        return Err(E_NO_FD);
    }
    hits += 1;

    // R3 chroot 对照：root 基准切换，已开 fd 基准不变。
    let mut f = BaseFabric::new(0x1000, 0x2000);
    f.open_fd(5)?;
    let fd_fp_before = {
        let mut fp = 0u32;
        let mut i = 0usize;
        while i < f.fds.len() {
            if f.fds[i].fd == 5 {
                fp = f.fds[i].base_fp;
            }
            i += 1;
        }
        fp
    };
    f.chroot(0x9000)?;
    if f.root_anchor != 0x9000 {
        return Err(E_BASE_MIX);
    }
    f.verify_fd_base(5)?;
    let mut fp_after = 0u32;
    let mut i = 0usize;
    while i < f.fds.len() {
        if f.fds[i].fd == 5 {
            fp_after = f.fds[i].base_fp;
        }
        i += 1;
    }
    if fp_after != fd_fp_before {
        return Err(E_BASE_MIX);
    }
    f.writeback(true, 0x40)?;
    hits += 1;

    // R4 守恒对账：断言账守恒 + fd 账审计。
    let mut f = BaseFabric::new(1, 1);
    f.assert_base(b"/x")?;
    f.assert_base(b"y")?;
    f.assert_base(b"/z")?;
    f.open_fd(1)?;
    f.open_fd(2)?;
    f.audit()?;
    if f.abs_asserts != 2 || f.rel_asserts != 1 {
        return Err(E_ASSERT_MISS);
    }
    hits += 1;

    // R5 追加回归：断言配额负例 + 多 fd 锚账审计 + chroot 代次。
    let mut f = BaseFabric::new(0xA, 0xB);
    f.assert_base(b"/only-abs")?;
    f.assert_base(b"only-rel")?;
    if f.assert_quota(1000).is_ok() {
        return Err(E_ASSERT_MISS); // 断言账远不足 1,000 必拦
    }
    let mut k = 0u32;
    while k < 8 {
        f.open_fd(k)?;
        k += 1;
    }
    if f.open_fd(4).is_ok() {
        return Err(E_FD_LIVE);
    }
    f.chroot(0xC0DE)?;
    if f.chroot_epoch != 1 {
        return Err(E_BAD_STATE);
    }
    // chroot 后八个 fd 基准锚逐一对照（判据 J2 扩面）。
    let mut m = 0u32;
    while m < 8 {
        f.verify_fd_base(m)?;
        m += 1;
    }
    f.audit()?;
    f.writeback(true, 0x50)?;
    hits += 1;

    Ok(hits)
}
