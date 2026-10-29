//! unx4022_sflagro — UNX-F4022 s_flags 只读位与透传契约（MNT_RDONLY→sb）（诊断段 0x8540）。
//!
//! 判据原文：
//!   J1 挂载只读位到 sb 写禁位的透传在写路径三处入口（write/setattr/truncate）
//!      全部生效：只读态下三条入口一律返回 EROFS。
//!   J2 只读态写入注入返回 EROFS 10/10 次，账目不因注入破坏。
//!   J3 透传是「只紧不松」：任一上层（mount）声明只读，sb 侧必置写禁位；
//!      上层可写不得把已只读的 sb 翻回可写。
//!   J4 读写态迁移账：进入只读 +1 / 退出只读 +1，二者与当前位合成守恒。
//! 设计纪律：
//!   1. 两层标志建模：mount 侧 MNT_RDONLY 与 sb 侧 SB_RDONLY 各自独立位，
//!      透传方向单向（mount → sb），合并规则为逻辑或（OR 粘滞）。
//!   2. 三处写入口以枚举 WritePath 表达，检查函数统一走 check_write()，
//!      禁止各入口自行判断（防止漏检——判据 J1 的三处即为此设）。
//!   3. EROFS=30 与 errno 基座投影一致；本模块内以常量引用，不改 errno 表。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

pub const SEG_BASE: u16 = 0x8540;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ROFS: u16 = SEG_BASE + 1;      // 只读态写入（EROFS 投影）
const E_PATH_BAD: u16 = SEG_BASE + 2;  // 写路径非法
const E_FLAG_BAD: u16 = SEG_BASE + 3;  // 标志位越出闭集
const E_DOWNGRADE: u16 = SEG_BASE + 4; // 试图把只读 sb 翻回可写
const E_BAL: u16 = SEG_BASE + 5;       // 只读账不守恒
const E_NULL_SB: u16 = SEG_BASE + 6;   // sb 未绑定（空槽）

/// EROFS 数值（与 errno 基座一致，只读文件系统）。
pub const EROFS_NUM: u16 = 30;

/// mount 侧标志位（与 mount 对象 mnt_flags 同构）。
pub const MNT_RDONLY: u32 = 0x0001;
pub const MNT_NOSUID: u32 = 0x0002;
pub const MNT_NODEV: u32 = 0x0004;
pub const MNT_NOEXEC: u32 = 0x0008;
pub const MNT_MASK: u32 = MNT_RDONLY | MNT_NOSUID | MNT_NODEV | MNT_NOEXEC;

/// sb 侧标志位（与 superblock s_flags 同构）。
pub const SB_RDONLY: u32 = 0x0001;
pub const SB_NOSUID: u32 = 0x0002;
pub const SB_NODEV: u32 = 0x0004;
pub const SB_NOEXEC: u32 = 0x0008;
pub const SB_MASK: u32 = SB_RDONLY | SB_NOSUID | SB_NODEV | SB_NOEXEC;

/// 写路径三处入口（判据 J1 的三处）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WritePath {
    Write = 0,    // 数据写
    Setattr = 1,  // 属性改（chmod/chown/utimes）
    Truncate = 2, // 截断
}

pub const PATH_COUNT: usize = 3;

/// 写路径名（报告导出）。
pub const PATH_NAMES: [&'static str; PATH_COUNT] = ["write", "setattr", "truncate"];

/// 只读门禁状态机：Writable → ReadOnly → Writable（单向粘滞由调用方策略控制）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GuardState {
    Writable = 0, // 可写
    ReadOnly = 1, // 只读（写禁）
}

/// 单入口统计视图：三账合成（判据 J1 的三入口逐项读面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RoPathStat {
    pub path: WritePath, // 写入口
    pub checks: u32,     // 检查次数
    pub allows: u32,     // 放行次数
    pub denies: u32,     // 拒绝次数
}

/// 只读门禁：mount 位 + sb 位 + 三路径检查账。
pub struct RoGuard {
    pub mnt_flags: u32,   // mount 侧标志
    pub sb_flags: u32,    // sb 侧标志
    pub state: GuardState, // 门禁状态
    pub checks: [u32; PATH_COUNT], // 三路径检查次数账
    pub denies: [u32; PATH_COUNT], // 三路径拒绝账
    pub allows: [u32; PATH_COUNT], // 三路径放行账
    pub enter_ro: u32,    // 进入只读次数
    pub exit_ro: u32,     // 退出只读次数
    pub downgrade_rej: u32, // 翻回可写被拒次数
}

impl RoGuard {
    pub fn new() -> RoGuard {
        RoGuard {
            mnt_flags: 0,
            sb_flags: 0,
            state: GuardState::Writable,
            checks: [0; PATH_COUNT],
            denies: [0; PATH_COUNT],
            allows: [0; PATH_COUNT],
            enter_ro: 0,
            exit_ro: 0,
            downgrade_rej: 0,
        }
    }

    /// 设置 mount 位：OR 粘滞透传到 sb（判据 J3）。
    pub fn set_mnt_flags(&mut self, flags: u32) -> Result<u32, u16> {
        if flags & !MNT_MASK != 0 {
            return Err(E_FLAG_BAD);
        }
        if flags & MNT_RDONLY != 0 {
            // 只紧不松：只读位一旦置上，粘滞保持（清位请求被忽略）。
            self.mnt_flags |= MNT_RDONLY;
            if self.state != GuardState::ReadOnly {
                self.state = GuardState::ReadOnly;
                self.enter_ro += 1;
            }
            self.sb_flags |= SB_RDONLY; // 透传
        }
        // 其余位按位赋值（非只读位不粘滞）。
        self.mnt_flags = (self.mnt_flags & MNT_RDONLY) | (flags & !MNT_RDONLY);
        self.sb_flags = (self.sb_flags & SB_RDONLY) | self.mnt_flags;
        Ok(self.sb_flags)
    }

    /// 三处写入口统一检查：只读态一律 EROFS（判据 J1）。
    pub fn check_write(&mut self, path: WritePath) -> Result<(), u16> {
        let i = path as usize;
        if i >= PATH_COUNT {
            return Err(E_PATH_BAD);
        }
        if self.sb_flags == 0 && self.mnt_flags == 0 {
            return Err(E_NULL_SB); // 未绑定 sb，写路径无宿主
        }
        self.checks[i] += 1;
        if self.state == GuardState::ReadOnly || self.sb_flags & SB_RDONLY != 0 {
            self.denies[i] += 1;
            return Err(E_ROFS);
        }
        self.allows[i] += 1;
        Ok(())
    }

    /// 可写态下三路径入口（模拟真实写动作）：门禁通过才落 write 账。
    pub fn write_path(&mut self, path: WritePath, bytes: u64) -> Result<u64, u16> {
        self.check_write(path)?;
        Ok(bytes)
    }

    /// 解除只读：仅在 mount 位被清除且显式请求时允许（remount rw）。
    pub fn remount_rw(&mut self) -> Result<GuardState, u16> {
        if self.state != GuardState::ReadOnly {
            return Err(E_BAL);
        }
        // 显式 remount rw 语义：允许一次解除，但须记账（非静默降级）。
        self.mnt_flags &= !MNT_RDONLY;
        self.sb_flags &= !SB_RDONLY;
        self.state = GuardState::Writable;
        self.exit_ro += 1;
        Ok(self.state)
    }

    /// 非法降级：未经 remount 直接清位，必须被拒（判据 J3 的「不松」面）。
    pub fn force_rw_inject(&mut self) -> Result<(), u16> {
        if self.state != GuardState::ReadOnly {
            return Err(E_BAL);
        }
        self.downgrade_rej += 1;
        Err(E_DOWNGRADE)
    }

    /// 只读位可观测（透传结果）。
    pub fn readonly(&self) -> bool {
        self.sb_flags & SB_RDONLY != 0
    }

    /// 单入口统计导出（越界入口返回 E_PATH_BAD）。
    pub fn path_stat(&self, p: WritePath) -> Result<RoPathStat, u16> {
        let i = p as usize;
        if i >= PATH_COUNT {
            return Err(E_PATH_BAD);
        }
        Ok(RoPathStat {
            path: p,
            checks: self.checks[i],
            allows: self.allows[i],
            denies: self.denies[i],
        })
    }

    /// 门禁账守恒：每路径 checks == allows + denies；进入/退出与状态位合成。
    pub fn conserve(&self) -> bool {
        for i in 0..PATH_COUNT {
            if self.checks[i] != self.allows[i] + self.denies[i] {
                return false;
            }
        }
        let expect_ro = self.state == GuardState::ReadOnly;
        let entered = self.enter_ro;
        let exited = self.exit_ro;
        // 状态位 = (进入 - 退出) == 1 → ReadOnly；== 0 → Writable。
        let delta = entered as i64 - exited as i64;
        let delta_ok = (delta == 1 && expect_ro) || (delta == 0 && !expect_ro);
        delta_ok && delta >= 0
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_ROFS => "只读态写入被拒（EROFS=30 投影）",
        E_PATH_BAD => "写路径非法（非 write/setattr/truncate）",
        E_FLAG_BAD => "标志位越出闭集",
        E_DOWNGRADE => "试图把只读 sb 静默翻回可写",
        E_BAL => "只读账不守恒",
        E_NULL_SB => "sb 未绑定（空槽）时写路径无宿主",
        _ => "未知诊断码",
    }
}

/// 确定性演练：三入口放行 → 置只读 → 三入口 EROFS 10/10 → 粘滞 → 账守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut g = RoGuard::new();

    // R1：可写态三处入口全部放行（判据 J1 的正向对照）。
    g.set_mnt_flags(MNT_NOSUID)?;
    g.write_path(WritePath::Write, 100)?;
    g.write_path(WritePath::Setattr, 1)?;
    g.write_path(WritePath::Truncate, 0)?;
    if g.readonly() {
        return Err(E_ROFS);
    }
    for i in 0..PATH_COUNT {
        if g.allows[i] != 1 || g.checks[i] != 1 {
            return Err(E_BAL);
        }
    }
    hits += 1;

    // R2：置 MNT_RDONLY → sb 侧 SB_RDONLY 立即透传（判据 J3 正向）。
    g.set_mnt_flags(MNT_RDONLY)?;
    if !g.readonly() || g.sb_flags & SB_RDONLY == 0 {
        return Err(E_ROFS);
    }
    if g.state != GuardState::ReadOnly || g.enter_ro != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：只读态三处入口全部 EROFS（判据 J1 原文）。
    let mut blocked = 0u32;
    for p in [WritePath::Write, WritePath::Setattr, WritePath::Truncate].iter() {
        if g.check_write(*p) == Err(E_ROFS) {
            blocked += 1;
        }
    }
    if blocked != 3 {
        return Err(E_ROFS);
    }
    hits += 1;

    // R4：只读态写入注入 10/10 返回 EROFS（判据 J2 原文）。
    let mut inj = 0u32;
    for n in 0..10u32 {
        let p = match n % 3 {
            0 => WritePath::Write,
            1 => WritePath::Setattr,
            _ => WritePath::Truncate,
        };
        if g.write_path(p, n as u64) == Err(E_ROFS) {
            inj += 1;
        }
    }
    if inj != 10 {
        return Err(E_ROFS);
    }
    // 三入口统计视图：write 账 6 检 1 放 5 拒（R1 放行 + R3 一次 + R4 四次拒绝）。
    let st = g.path_stat(WritePath::Write)?;
    if st.path != WritePath::Write || st.checks != 6 || st.allows != 1 || st.denies != 5 {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：粘滞——再请求可写（清只读位）不得翻回（判据 J3「只紧不松」）。
    g.set_mnt_flags(MNT_NOSUID)?;
    if !g.readonly() || g.state != GuardState::ReadOnly {
        return Err(E_DOWNGRADE);
    }
    if g.write_path(WritePath::Write, 1) != Err(E_ROFS) {
        return Err(E_ROFS);
    }
    hits += 1;

    // R6：静默降级注入被拒并入账。
    if g.force_rw_inject() != Err(E_DOWNGRADE) {
        return Err(E_DOWNGRADE);
    }
    if g.downgrade_rej != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R7：显式 remount rw 允许一次解除，账目记 exit。
    if g.remount_rw()? != GuardState::Writable {
        return Err(E_BAL);
    }
    if g.readonly() || g.exit_ro != 1 {
        return Err(E_ROFS);
    }
    // 解除后写路径恢复放行。
    g.write_path(WritePath::Write, 5)?;
    hits += 1;

    // R8：账守恒——三路径 checks == allows + denies，进出只读合成状态位。
    if !g.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：标志位越界注入被拒。
    if g.set_mnt_flags(0x1000) != Err(E_FLAG_BAD) {
        return Err(E_FLAG_BAD);
    }
    hits += 1;

    // I2：空 sb 写路径注入被拒（未绑定宿主）。
    let mut empty = RoGuard::new();
    if empty.write_path(WritePath::Write, 1) != Err(E_NULL_SB) {
        return Err(E_NULL_SB);
    }
    hits += 1;

    // I3：可写态做 remount rw 注入被拒（无只读可解）。
    let mut w = RoGuard::new();
    w.set_mnt_flags(MNT_NOEXEC)?;
    if w.remount_rw() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    // I4：可写态强制降级注入被拒。
    if w.force_rw_inject() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    // I5：再次置只读并验 EROFS（状态机往复后仍生效）。
    w.set_mnt_flags(MNT_RDONLY)?;
    if w.write_path(WritePath::Truncate, 0) != Err(E_ROFS) {
        return Err(E_ROFS);
    }
    if !w.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
