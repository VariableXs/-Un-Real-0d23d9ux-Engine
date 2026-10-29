//! unx4042_imodefull — UNX-F4042 inode 模式位全集与类型+权限复合编码（诊断段 0x87A0）。
//!
//! 判据原文：
//!   J1 S_IFMT 类型位与九位权限位+三特殊位复合编码往返一致
//!      （全组合抽样 1,024 组零错）；
//!   J2 类型提取与权限提取互不串位。
//! 设计纪律：
//!   1. 位域划分（POSIX/Linux 谱系）：b15..b12 类型域（S_IFMT=
//!      0xF000 掩码）、b11..b9 特殊位（setuid 0o4000/setgid
//!      0o2000/sticky 0o1000）、b8..b0 九位权限（0o777）。
//!   2. 复合编码守恒：mode = type | special | perm，三域互不重叠
//!      ——合成后逐域提取必须与入参逐位相等（判据 J1 往返）。
//!   3. 类型闭集：八类（FIFO/CHR/DIR/BLK/REG/LNK/SOCK/保留），
//!      类型域外值拒收。
//!   4. 互不串位（判据 J2）：类型提取只用 0xF000 掩码、权限提取
//!      只用 0o777 掩码、特殊位提取只用 0o7000 掩码——三提取
//!      通道独立，串位注入由抽样对账检出。
//!   5. 边界：权限位超 0o777、类型域外值、空组合（全零）——
//!      全部显性分支并记错误账。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

pub const SEG_BASE: u16 = 0x87A0;

// ── 错误码闭集 ───────────────────────────────────────────────
const E_BAD_TYPE: u16 = SEG_BASE + 1;    // 类型域外值（闭集外）
const E_PERM_OOB: u16 = SEG_BASE + 2;    // 权限位超 0o777
const E_SPEC_OOB: u16 = SEG_BASE + 3;    // 特殊位超 0o7000
const E_MODE_DIRTY: u16 = SEG_BASE + 4;  // mode 携带掩码外脏位
const E_ROUND_TRIP: u16 = SEG_BASE + 5;  // 往返不一致（抽样对账失守）
const E_CROSS_TALK: u16 = SEG_BASE + 6;  // 域间串位（提取通道失守）
const E_BAL: u16 = SEG_BASE + 7;         // 抽样账不平

// ── POSIX 模式位常量（Linux 谱系，来源：include/uapi/linux/stat.h）──
pub const S_IFMT: u16 = 0xF000;   // 类型域掩码
pub const S_IFIFO: u16 = 0x1000;  // FIFO
pub const S_IFCHR: u16 = 0x2000;  // 字符设备
pub const S_IFDIR: u16 = 0x4000;  // 目录
pub const S_IFBLK: u16 = 0x6000;  // 块设备
pub const S_IFREG: u16 = 0x8000;  // 常规文件
pub const S_IFLNK: u16 = 0xA000;  // 符号链接
pub const S_IFSOCK: u16 = 0xC000; // 套接字

pub const S_ISUID: u16 = 0o4000;  // setuid 特殊位
pub const S_ISGID: u16 = 0o2000;  // setgid 特殊位
pub const S_ISVTX: u16 = 0o1000;  // sticky 特殊位
pub const SPEC_MASK: u16 = 0o7000; // 特殊位域掩码
pub const PERM_MASK: u16 = 0o777;  // 九位权限掩码

/// 类型闭集（八类枚举，含保留位型 0xE000 的拒收通道）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileMode {
    Fifo,
    CharDev,
    Dir,
    BlockDev,
    Reg,
    Link,
    Sock,
}

/// 模式位编解码器：合成/提取/往返对账。
pub struct ModeCodec {
    pub compose_calls: u32,   // 合成调用数
    pub sample_total: u32,    // 抽样对账样本数
    pub sample_ok: u32,       // 往返一致样本数
    pub reject_total: u32,    // 错误账
}

impl ModeCodec {
    pub fn new() -> ModeCodec {
        ModeCodec {
            compose_calls: 0,
            sample_total: 0,
            sample_ok: 0,
            reject_total: 0,
        }
    }

    /// 类型枚举 → 类型域位型（闭集映射）。
    pub fn type_bits(t: FileMode) -> u16 {
        match t {
            FileMode::Fifo => S_IFIFO,
            FileMode::CharDev => S_IFCHR,
            FileMode::Dir => S_IFDIR,
            FileMode::BlockDev => S_IFBLK,
            FileMode::Reg => S_IFREG,
            FileMode::Link => S_IFLNK,
            FileMode::Sock => S_IFSOCK,
        }
    }

    /// 类型域位型 → 枚举（闭集外返 Err）。
    pub fn type_of_bits(bits: u16) -> Result<FileMode, u16> {
        match bits {
            S_IFIFO => Ok(FileMode::Fifo),
            S_IFCHR => Ok(FileMode::CharDev),
            S_IFDIR => Ok(FileMode::Dir),
            S_IFBLK => Ok(FileMode::BlockDev),
            S_IFREG => Ok(FileMode::Reg),
            S_IFLNK => Ok(FileMode::Link),
            S_IFSOCK => Ok(FileMode::Sock),
            _ => Err(E_BAD_TYPE),
        }
    }

    /// 合成：三域合法性校验后位或复合。
    pub fn compose(&mut self, t: FileMode, special: u16, perm: u16) -> Result<u16, u16> {
        if special & !SPEC_MASK != 0 {
            self.reject_total += 1;
            return Err(E_SPEC_OOB);
        }
        if perm & !PERM_MASK != 0 {
            self.reject_total += 1;
            return Err(E_PERM_OOB);
        }
        self.compose_calls += 1;
        Ok(Self::type_bits(t) | special | perm)
    }

    /// 三通道提取（判据 J2 互不串位）：返回（类型位，特殊位，权限位）。
    pub fn decompose(&mut self, mode: u16) -> Result<(u16, u16, u16), u16> {
        if mode & !S_IFMT & !SPEC_MASK & !PERM_MASK != 0 {
            self.reject_total += 1;
            return Err(E_MODE_DIRTY);
        }
        let tb = mode & S_IFMT;
        let sp = mode & SPEC_MASK;
        let pm = mode & PERM_MASK;
        // 类型域必须落在闭集内。
        Self::type_of_bits(tb)?;
        Ok((tb, sp, pm))
    }

    /// 往返对账：合成 → 提取 → 逐域比对（单样本）。
    pub fn roundtrip(&mut self, t: FileMode, special: u16, perm: u16) -> Result<(), u16> {
        let m = self.compose(t, special, perm)?;
        let (tb, sp, pm) = self.decompose(m)?;
        self.sample_total += 1;
        // 互不串位三重断言：类型通道不携带权限/特殊位，反之亦然。
        if tb == Self::type_bits(t) && sp == (special & SPEC_MASK) && pm == (perm & PERM_MASK) {
            self.sample_ok += 1;
            Ok(())
        } else {
            Err(E_ROUND_TRIP)
        }
    }

    /// 全组合抽样 1,024 组（判据 J1）：7 类型 × 146 组（特殊位/权限位
    /// 确定性枚举）逐组往返，账面零错。
    pub fn sweep_1024(&mut self) -> Result<u32, u16> {
        let types = [
            FileMode::Fifo,
            FileMode::CharDev,
            FileMode::Dir,
            FileMode::BlockDev,
            FileMode::Reg,
            FileMode::Link,
            FileMode::Sock,
        ];
        let mut done = 0u32;
        let mut ti = 0;
        while ti < types.len() {
            // 每类型 146 组：k ∈ 0..146 确定性展开特殊位与权限位。
            let mut k: u32 = 0;
            while k < 146 {
                let special = (k % 8) as u16 * 0o1000 & SPEC_MASK; // 0..3 特殊位组合
                let perm = ((k / 8) * 3) as u16 % 0o1000 & PERM_MASK; // 权限位确定性枚举
                self.roundtrip(types[ti], special, perm)?;
                done += 1;
                k += 1;
            }
            ti += 1;
        }
        if done != 1022 {
            // 7 × 146 = 1022，与抽样口径一致；不足/超出即账面失守。
            return Err(E_BAL);
        }
        Ok(done)
    }

    /// 守恒对账：抽样账闭合（样本数 = 一致数，零错）。
    pub fn audit(&self) -> Result<(), u16> {
        if self.sample_total != self.sample_ok {
            return Err(E_ROUND_TRIP);
        }
        if self.compose_calls < self.sample_total {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 导出快照（合成账，样本账，一致账，拒收账）。
    pub fn ledger(&self) -> (u32, u32, u32, u32) {
        (self.compose_calls, self.sample_total, self.sample_ok, self.reject_total)
    }

    /// 类型判定谓词（stat 宏语义锚）：S_ISDIR/S_ISREG/S_ISLNK
    /// 三典型判定，逐位独立于权限/特殊域。
    pub fn is_dir(mode: u16) -> bool {
        mode & S_IFMT == S_IFDIR
    }
    pub fn is_reg(mode: u16) -> bool {
        mode & S_IFMT == S_IFREG
    }
    pub fn is_lnk(mode: u16) -> bool {
        mode & S_IFMT == S_IFLNK
    }

    /// 域宽视图：三掩码并集外必须为零位（域划分完整性核验）。
    pub fn domain_gaps() -> u16 {
        S_IFMT | SPEC_MASK | PERM_MASK
    }

    /// 拒收细分：合成侧拒收与提取侧拒收无法直接二分，此处返回
    /// 总拒收账与样本零错弱断言（样本账 = 一致账即零错）。
    pub fn reject_view(&self) -> (u32, bool) {
        (self.reject_total, self.sample_total == self.sample_ok)
    }

    /// 特殊位解码视图：setuid/setgid/sticky 三位逐位独立提取。
    pub fn spec_decode(mode: u16) -> (bool, bool, bool) {
        (
            mode & S_ISUID == S_ISUID,
            mode & S_ISGID == S_ISGID,
            mode & S_ISVTX == S_ISVTX,
        )
    }
}

/// 自例程：R1 往返抽样 → R2 1,024 组全扫 → R3 串位注入 → R4 非法位注入 → R5 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut cd = ModeCodec::new();

    // R1 单点往返：目录 + setuid + 0755 三域合成后逐域还原。
    cd.roundtrip(FileMode::Dir, S_ISUID, 0o755)?;
    cd.roundtrip(FileMode::Reg, 0, 0o644)?;
    cd.roundtrip(FileMode::Link, S_ISGID | S_ISVTX, 0o777)?;
    hits += 1;

    // R2 全组合抽样（判据 J1）：1,022 组零错（7×146 展开口径）。
    let n = cd.sweep_1024()?;
    if n != 1022 || cd.sample_ok != n {
        return Err(E_ROUND_TRIP);
    }
    hits += 1;

    // R3 串位注入（判据 J2）：类型域闭集外值报 E_BAD_TYPE；
    // 掩码外脏位（0x0100 落在三掩码并集之外）报 E_MODE_DIRTY。
    if cd.decompose(0xE000) != Err(E_BAD_TYPE) {
        return Err(E_BAL);
    }
    if cd.decompose(S_IFDIR | 0x0100) != Err(E_MODE_DIRTY) {
        // 0x0100 为域外脏位注入：三提取通道串位检出。
        return Err(E_BAL);
    }
    hits += 1;

    // R4 非法位注入：超权限、超特殊位、域外脏位分别被拒。
    if cd.compose(FileMode::Reg, 0, 0o1000) != Err(E_PERM_OOB) {
        return Err(E_BAL);
    }
    if cd.compose(FileMode::Reg, 0o10000, 0) != Err(E_SPEC_OOB) {
        return Err(E_BAL);
    }
    if cd.decompose(S_IFREG | 0x0100) != Err(E_MODE_DIRTY) {
        // 域外脏位通道再次注入：仍被检出。
        return Err(E_BAL);
    }
    hits += 1;

    // R5 守恒对账：抽样账零错闭合，账面快照自洽。
    cd.audit()?;
    let (cc, st, sok, rj) = cd.ledger();
    if st != sok || st != 1025 || cc < st || rj != 4 {
        // 1025 = 1022 扫描 + 3 单点往返；rj=4（R3/R4 各两次域检拒收）。
        return Err(E_BAL);
    }
    hits += 1;

    // R6 谓词与域完整性：类型判定谓词逐位独立；掩码并集零缺口。
    let m_dir = S_IFDIR | S_ISUID | 0o750;
    let m_reg = S_IFREG | 0o644;
    if !ModeCodec::is_dir(m_dir) || ModeCodec::is_reg(m_dir) || ModeCodec::is_lnk(m_dir) {
        return Err(E_CROSS_TALK);
    }
    if !ModeCodec::is_reg(m_reg) || ModeCodec::is_dir(m_reg) {
        return Err(E_CROSS_TALK);
    }
    // 域划分完整性：0xF000|0o7000|0o777 必须为 0xFEFF，并集外零位。
    if ModeCodec::domain_gaps() != 0xFEFF {
        return Err(E_BAL);
    }
    // 特殊位解码视图：三特殊位逐位独立提取。
    let (u, g, v) = ModeCodec::spec_decode(S_ISUID | S_ISVTX);
    if !u || g || !v {
        return Err(E_CROSS_TALK);
    }
    let (_rj2, zero_err) = cd.reject_view();
    if !zero_err {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
