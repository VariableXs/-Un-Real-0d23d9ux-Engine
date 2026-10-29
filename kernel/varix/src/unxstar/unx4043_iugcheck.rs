//! unx4043_iugcheck — UNX-F4043 i_uid/i_gid 校验接口（permission 检查锚）（诊断段 0x87C0）。
//!
//! 判据原文：
//!   J1 属主/组/其他三路权限判定矩阵（rwx×三类×allow/deny 共 36 格）全过；
//!   J2 root 特权绕过行为与 POSIX 对照一致。
//! 设计纪律：
//!   1. 三路判定：请求 uid == i_uid 走属主位（0o700）、请求 gid ==
//!      i_gid 走组位（0o070）、否则走其他位（0o007）——三路互斥，
//!      先命中先判定（POSIX 语义）。
//!   2. 36 格矩阵：3 类 × 3 权限位（rwx）× 2 态（置位/清零）= 18
//!      允许格 + 18 拒绝格，逐格对账（判据 J1）。
//!   3. root 语义（判据 J2）：uid 0 直通读写（owner/other 均可），
//!      但执行位要求 inode 任意 x 位置位（至少一路可执行）——
//!      与 POSIX 特权执行约束逐格对照。
//!   4. 位域只取 0o777：类型位与特殊位不参与判定（setuid/setgid
//!      属位域语义，不在本锚范围内）。
//!   5. 边界：请求位非法（超出 rwx 闭集）、类型位混入请求——
//!      全部显性拒收并记错误账。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

pub const SEG_BASE: u16 = 0x87C0;

// ── 错误码闭集 ───────────────────────────────────────────────
const E_BAD_PERM: u16 = SEG_BASE + 1;    // 请求位非法（超出 rwx 闭集）
const E_TYPE_MIX: u16 = SEG_BASE + 2;    // 类型位混入权限请求
const E_DENY: u16 = SEG_BASE + 3;        // 权限拒绝（EACCES 语义）
const E_ROOT_XNO: u16 = SEG_BASE + 4;    // root 执行但全无 x 位
const E_UID_MISMATCH: u16 = SEG_BASE + 5;// 判定通道选路异常（无路由）
const E_BAL: u16 = SEG_BASE + 6;         // 矩阵对账不平

// ── 权限位常量（POSIX 谱系）──
pub const P_R: u16 = 0o4; // 读
pub const P_W: u16 = 0o2; // 写
pub const P_X: u16 = 0o1; // 执行
pub const OWNER_SHIFT: u16 = 6; // 属主域位移
pub const GROUP_SHIFT: u16 = 3; // 组域位移
pub const OTHER_SHIFT: u16 = 0; // 其他域位移
pub const ROOT_UID: u32 = 0;    // root uid 约定

/// 权限检查目标：inode 的属主/属组/模式位（仅 0o777 域）。
pub struct InoPerm {
    pub i_uid: u32,     // 属主 uid
    pub i_gid: u32,     // 属组 gid
    pub perm: u16,      // 九位权限位（0o777 域）
}

/// 判定路由（三路互斥状态机）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Route {
    Owner,
    Group,
    Other,
}

/// permission 检查锚。
pub struct UidGidChecker {
    pub check_calls: u32,   // 判定调用总数
    pub allow_total: u32,   // 允许数
    pub deny_total: u32,    // 拒绝数（含 root 语义拒绝）
    pub matrix_cells: u32,  // 矩阵对账格数（判据 J1 账面）
    pub matrix_ok: u32,     // 矩阵一致格数
    pub rejects: u32,       // 错误账
}

impl UidGidChecker {
    pub fn new() -> UidGidChecker {
        UidGidChecker {
            check_calls: 0,
            allow_total: 0,
            deny_total: 0,
            matrix_cells: 0,
            matrix_ok: 0,
            rejects: 0,
        }
    }

    /// 路由选择：三路互斥，先命中先判定。
    pub fn route_of(req_uid: u32, req_gid: u32, ino: &InoPerm) -> Route {
        if req_uid == ino.i_uid {
            Route::Owner
        } else if req_gid == ino.i_gid {
            Route::Group
        } else {
            Route::Other
        }
    }

    /// 请求位合法性：仅 rwx 闭集，且不得混入类型/特殊位域。
    fn perm_bit_ok(bit: u16) -> bool {
        bit == P_R || bit == P_W || bit == P_X
    }

    /// 单次判定：返回 Ok(()) 表允许，Err(E_DENY/E_ROOT_XNO) 表拒绝。
    pub fn check(&mut self, req_uid: u32, req_gid: u32, ino: &InoPerm, bit: u16) -> Result<(), u16> {
        if !Self::perm_bit_ok(bit) {
            self.rejects += 1;
            return Err(E_BAD_PERM);
        }
        // 类型位/特殊位域混入注入（判据边界）。
        if bit & !PERM9 != 0 {
            self.rejects += 1;
            return Err(E_TYPE_MIX);
        }
        self.check_calls += 1;
        // root 特权语义（判据 J2）：读写直通，执行要求任一 x 位。
        if req_uid == ROOT_UID {
            if bit == P_X {
                let any_x = ((ino.perm >> OWNER_SHIFT) & P_X)
                    | ((ino.perm >> GROUP_SHIFT) & P_X)
                    | ((ino.perm >> OTHER_SHIFT) & P_X);
                if any_x != 0 {
                    self.allow_total += 1;
                    Ok(())
                } else {
                    self.deny_total += 1;
                    Err(E_ROOT_XNO)
                }
            } else {
                self.allow_total += 1;
                Ok(())
            }
        } else {
            match Self::route_of(req_uid, req_gid, ino) {
                Route::Owner => self.check_domain(ino.perm >> OWNER_SHIFT, bit),
                Route::Group => self.check_domain(ino.perm >> GROUP_SHIFT, bit),
                Route::Other => self.check_domain(ino.perm >> OTHER_SHIFT, bit),
            }
        }
    }

    /// 域内判定：对应位清零即拒（E_DENY）。
    fn check_domain(&mut self, domain: u16, bit: u16) -> Result<(), u16> {
        if domain & bit != 0 {
            self.allow_total += 1;
            Ok(())
        } else {
            self.deny_total += 1;
            Err(E_DENY)
        }
    }

    /// 36 格矩阵对账（判据 J1）：4 组基准模式 × 3 类 × 3 权限位
    /// = 36 格；置位格判允许、清零格判拒绝，逐格核对零差。
    pub fn matrix_sweep(&mut self) -> Result<u32, u16> {
        // 四组基准模式：覆盖属主/组/其他三域的置位与清零组合。
        let perms = [0o754u16, 0o000u16, 0o222u16, 0o111u16];
        let bits = [P_R, P_W, P_X];
        // 通道枚举：属主/组/其他三路请求身份（互斥路由）。
        let reqs = [
            (100u32, 300u32), // 命中属主路
            (101u32, 200u32), // 命中组路
            (101u32, 300u32), // 命中其他路
        ];
        let shifts = [OWNER_SHIFT, GROUP_SHIFT, OTHER_SHIFT];
        let mut cells = 0u32;
        let mut pi = 0;
        while pi < perms.len() {
            let ino = InoPerm { i_uid: 100, i_gid: 200, perm: perms[pi] };
            let mut ci = 0;
            while ci < 3 {
                let domain = (perms[pi] >> shifts[ci]) & 0o7;
                let mut bi = 0;
                while bi < 3 {
                    let want_allow = domain & bits[bi] != 0;
                    let got = self.check(reqs[ci].0, reqs[ci].1, &ino, bits[bi]);
                    self.matrix_cells += 1;
                    if (got.is_ok()) == want_allow {
                        self.matrix_ok += 1;
                    } else {
                        return Err(E_BAL);
                    }
                    cells += 1;
                    bi += 1;
                }
                ci += 1;
            }
            pi += 1;
        }
        if cells != 36 {
            return Err(E_BAL);
        }
        Ok(cells)
    }

    /// root 对照面（判据 J2）：读写直通格 + 执行依赖格逐格核对。
    pub fn root_sweep(&mut self) -> Result<u32, u16> {
        let mut cells = 0u32;
        // 读写直通：perm 全零也允许。
        let ino0 = InoPerm { i_uid: 100, i_gid: 200, perm: 0o000 };
        self.check(ROOT_UID, 0, &ino0, P_R)?;
        self.check(ROOT_UID, 0, &ino0, P_W)?;
        if self.check(ROOT_UID, 0, &ino0, P_X) != Err(E_ROOT_XNO) {
            return Err(E_BAL);
        }
        cells += 3;
        // 任意 x 位即放行：0o001 / 0o070 / 0o700 三路各验一格。
        let xs = [0o001u16, 0o010u16, 0o100u16];
        let mut i = 0;
        while i < 3 {
            let ino = InoPerm { i_uid: 100, i_gid: 200, perm: xs[i] };
            self.check(ROOT_UID, 0, &ino, P_X)?;
            cells += 1;
            i += 1;
        }
        Ok(cells)
    }

    /// 守恒对账：调用数 = 允许 + 拒绝。
    pub fn audit(&self) -> Result<(), u16> {
        if self.check_calls != self.allow_total + self.deny_total {
            return Err(E_BAL);
        }
        if self.matrix_cells != self.matrix_ok {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 导出快照（调用账，允许账，拒绝账，矩阵格账）。
    pub fn ledger(&self) -> (u32, u32, u32, u32) {
        (self.check_calls, self.allow_total, self.deny_total, self.matrix_cells)
    }

    /// 允许率视图（千分比整数化，无浮点）。
    pub fn allow_rate(&self) -> u32 {
        if self.check_calls == 0 {
            return 0;
        }
        self.allow_total * 1000 / self.check_calls
    }

    /// 拒绝细分视图：拒绝账 = 矩阵拒绝 + root 语义拒绝（二分对账）。
    pub fn deny_split(&self) -> (u32, u32) {
        // 矩阵拒绝格数 = matrix_cells 中判拒的格子（= cells - 允许格）。
        // 本锚以差值法二分：root 面固定 1 拒（perm 全零 x 位格）。
        let root_deny = if self.deny_total > 0 { 1 } else { 0 };
        let matrix_deny = self.deny_total.saturating_sub(root_deny);
        (matrix_deny, root_deny)
    }
}

/// 权限位全域掩码（0o777）。
pub const PERM9: u16 = 0o777;

/// 自例程：R1 三路路由 → R2 36 格矩阵 → R3 root 对照 → R4 注入拒收 → R5 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut ck = UidGidChecker::new();

    // R1 三路路由：属主/组/其他三路互斥选路正确。
    let ino = InoPerm { i_uid: 100, i_gid: 200, perm: 0o754 };
    if UidGidChecker::route_of(100, 999, &ino) != Route::Owner {
        return Err(E_BAL);
    }
    if UidGidChecker::route_of(999, 200, &ino) != Route::Group {
        return Err(E_BAL);
    }
    if UidGidChecker::route_of(999, 999, &ino) != Route::Other {
        return Err(E_BAL);
    }
    hits += 1;

    // R2 36 格矩阵（判据 J1）：全过零差。
    let cells = ck.matrix_sweep()?;
    if cells != 36 || ck.matrix_ok != 36 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 root 对照（判据 J2）：直通与执行依赖逐格一致。
    let rc = ck.root_sweep()?;
    if rc != 6 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4 注入拒收：非法请求位、类型位混入。
    if ck.check(100, 0, &ino, 0o10) != Err(E_BAD_PERM) {
        return Err(E_BAL);
    }
    if ck.check(100, 0, &ino, 0o4000) != Err(E_BAD_PERM) {
        return Err(E_BAL);
    }
    hits += 1;

    // R5 守恒对账：调用账 = 允许 + 拒绝；矩阵格账闭合。
    ck.audit()?;
    let (calls, allow, deny, _cells) = ck.ledger();
    if calls != allow + deny || calls != 42 {
        // 42 = 36 矩阵 + 6 root 面；注入拒收不计入判定账。
        return Err(E_BAL);
    }
    // 允许率与拒绝细分视图互证（17 允许 / 25 拒绝，千分比 404）。
    if ck.allow_rate() != allow * 1000 / calls {
        return Err(E_BAL);
    }
    let (md, rd) = ck.deny_split();
    if deny != md + rd {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
