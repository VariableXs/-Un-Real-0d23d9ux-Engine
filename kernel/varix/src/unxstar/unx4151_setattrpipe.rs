//! unx4151_setattrpipe — UNX-F4151 setattr 管道统一约定（i_attr 结构）（诊断段 0x94A0）。
//!
//! 判据原文：
//!   J1 i_attr 位掩码结构（chmod/chown/truncate/touch 四类请求）统一入口断言通过。
//!   J2 掩码外位注入被拒。
//!   J3 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. i_attr 为位掩码闭集：MODE/UID/GID/SIZE/ATIME/MTIME 六有效位，
//!      四类请求各自映射到合法位组合，组合外任何一位都判非法。
//!   2. 统一入口单一管道：所有 setattr 请求经同一 apply() 进出，
//!      分支只按掩码位走，禁止旁路直写 inode 字段。
//!   3. 属性账守恒：每次 apply 记录 before/after 指纹，拒收请求
//!      必须零变更（指纹不变）。
//!   4. 边界显性：掩码 0（无事可做）、掩码超集（非法位）、truncate
//!      尺寸回涨语义按 POSIX 口径裁定并入账。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

pub const SEG_BASE: u16 = 0x94A0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_BAD_MASK: u16 = SEG_BASE + 1;    // 掩码含非法位（掩码外位注入）
const E_ZERO_MASK: u16 = SEG_BASE + 2;   // 掩码为 0（无事可做仍进管道）
const E_MODE_BITS: u16 = SEG_BASE + 3;   // mode 位超 12 位（0o7777）域
const E_SIZE_UP: u16 = SEG_BASE + 4;     // truncate 尺寸回涨拒收口径
const E_ATTR_MIX: u16 = SEG_BASE + 5;    // 请求类别与掩码位不匹配
const E_MUTATE_LEAK: u16 = SEG_BASE + 6; // 拒收请求却改动了指纹（判据失守）
const E_BAD_STATE: u16 = SEG_BASE + 7;   // 管道状态机非法迁移
const E_EMPTY_SUM: u16 = SEG_BASE + 8;   // 写回摘要为空
const E_BAL: u16 = SEG_BASE + 9;         // 四类请求计数账与落账总数不平

/// i_attr 有效位闭集（Linux inode_change 口径裁剪）。
pub const ATTR_MODE: u32 = 1 << 0;   // chmod：权限位
pub const ATTR_UID: u32 = 1 << 1;    // chown：属主
pub const ATTR_GID: u32 = 1 << 2;    // chown：属组
pub const ATTR_SIZE: u32 = 1 << 3;   // truncate：尺寸
pub const ATTR_ATIME: u32 = 1 << 4;  // touch：访问时间戳
pub const ATTR_MTIME: u32 = 1 << 5;  // touch：修改时间戳
pub const ATTR_VALID: u32 = 0x3F;    // 六位全集合

/// 四类请求类别（掩码组合模板）。
pub const REQ_CHMOD: u8 = 0;
pub const REQ_CHOWN: u8 = 1;
pub const REQ_TRUNCATE: u8 = 2;
pub const REQ_TOUCH: u8 = 3;
pub const REQ_COUNT: usize = 4;

/// 管道状态机：Idle 待命 / Applier 校验中 / Applied 已落账 / Rejected 已拒。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PipeState {
    Idle,
    Applier,
    Applied,
    Rejected,
}

/// 一次 setattr 请求（i_attr 结构本体）。
pub struct IAttr {
    pub mask: u32,       // 有效位掩码（只允许 ATTR_VALID 子集）
    pub mode: u32,       // chmod 目标权限位（<= 0o7777）
    pub uid: u32,        // chown 属主
    pub gid: u32,        // chown 属组
    pub size: u32,       // truncate 目标尺寸
    pub stamp_tick: u64, // touch 时间戳（整数时钟滴答，非墙钟）
}

/// inode 属性账：指纹与四类请求计数。
pub struct InodeAttr {
    pub mode: u32,       // 当前权限位
    pub uid: u32,        // 当前属主
    pub gid: u32,        // 当前属组
    pub size: u32,       // 当前尺寸
    pub stamp_tick: u64, // 当前时间戳
}

/// setattr 管道枢纽：统一入口 + 指纹账 + 拒收零变更守恒。
pub struct SetattrPipe {
    pub inode: InodeAttr,
    pub state: PipeState,
    pub req_counts: [u32; REQ_COUNT], // 四类请求成功计数
    pub rejected_total: u32,          // 拒收总数（含掩码外位注入）
    pub applied_total: u32,           // 落账总数
}

impl SetattrPipe {
    pub fn new(mode: u32, uid: u32, gid: u32, size: u32) -> SetattrPipe {
        SetattrPipe {
            inode: InodeAttr { mode, uid, gid, size, stamp_tick: 0 },
            state: PipeState::Idle,
            req_counts: [0; REQ_COUNT],
            rejected_total: 0,
            applied_total: 0,
        }
    }

    fn fingerprint(&self) -> u32 {
        // 整数指纹：五字段异或折叠（回滚零变更判定）。
        let mut h: u32 = 0x811C9DC5;
        h = h.wrapping_mul(0x01000193) ^ self.inode.mode;
        h = h.wrapping_mul(0x01000193) ^ self.inode.uid;
        h = h.wrapping_mul(0x01000193) ^ self.inode.gid;
        h = h.wrapping_mul(0x01000193) ^ self.inode.size;
        h = h.wrapping_mul(0x01000193) ^ (self.inode.stamp_tick as u32);
        h = h.wrapping_mul(0x01000193) ^ ((self.inode.stamp_tick >> 32) as u32);
        h
    }

    /// 请求类别→合法掩码组合闭集（判据 J1 的矩阵面）。
    pub fn req_mask_of(req: u8) -> Result<u32, u16> {
        match req {
            REQ_CHMOD => Ok(ATTR_MODE),
            REQ_CHOWN => Ok(ATTR_UID | ATTR_GID),
            REQ_TRUNCATE => Ok(ATTR_SIZE),
            REQ_TOUCH => Ok(ATTR_ATIME | ATTR_MTIME),
            _ => Err(E_ATTR_MIX),
        }
    }

    /// 统一入口：掩码校验 → 类别匹配 → 逐位落账；任一步失败即 Rejected
    /// 且零变更（判据 J2：掩码外位在此被拒）。
    pub fn apply(&mut self, req: u8, a: &IAttr) -> Result<(), u16> {
        self.state = PipeState::Applier;
        let fp_before = self.fingerprint();

        // 1) 掩码闭集：超 ATTR_VALID 的任何位都是注入。
        if a.mask & !ATTR_VALID != 0 {
            self.state = PipeState::Rejected;
            self.rejected_total += 1;
            return Err(E_BAD_MASK);
        }
        if a.mask == 0 {
            self.state = PipeState::Rejected;
            self.rejected_total += 1;
            return Err(E_ZERO_MASK);
        }
        // 2) 掩码必须恰好是请求类别的合法组合（多余有效位也拒）。
        let legal = Self::req_mask_of(req)?;
        if a.mask != legal {
            self.state = PipeState::Rejected;
            self.rejected_total += 1;
            return Err(E_ATTR_MIX);
        }
        // 3) 逐类校验与落账。
        match req {
            REQ_CHMOD => {
                if a.mode > 0o7777 {
                    self.state = PipeState::Rejected;
                    self.rejected_total += 1;
                    return Err(E_MODE_BITS);
                }
                self.inode.mode = a.mode;
            }
            REQ_CHOWN => {
                self.inode.uid = a.uid;
                self.inode.gid = a.gid;
            }
            REQ_TRUNCATE => {
                // POSIX truncate 口径：只缩不涨（本契约冻结此裁定）。
                if a.size > self.inode.size {
                    self.state = PipeState::Rejected;
                    self.rejected_total += 1;
                    return Err(E_SIZE_UP);
                }
                self.inode.size = a.size;
            }
            REQ_TOUCH => {
                self.inode.stamp_tick = a.stamp_tick;
            }
            _ => {
                self.state = PipeState::Rejected;
                self.rejected_total += 1;
                return Err(E_ATTR_MIX);
            }
        }
        self.state = PipeState::Applied;
        self.applied_total += 1;
        self.req_counts[req as usize] += 1;
        let _ = fp_before; // 成功路径允许变更
        Ok(())
    }

    /// 拒收后零变更断言：当前指纹必须等于调用方提供的 before 值。
    pub fn verify_unchanged(&self, fp_before: u32) -> Result<(), u16> {
        if self.fingerprint() != fp_before {
            return Err(E_MUTATE_LEAK);
        }
        Ok(())
    }

    pub fn fingerprint_pub(&self) -> u32 {
        self.fingerprint()
    }

    /// 守恒对账：四类成功计数合计 = 落账总数，状态机合法。
    pub fn audit(&self) -> Result<(), u16> {
        let sum = self.req_counts[0] + self.req_counts[1]
            + self.req_counts[2] + self.req_counts[3];
        if sum != self.applied_total {
            return Err(E_BAL);
        }
        match self.state {
            PipeState::Applied | PipeState::Idle | PipeState::Rejected => {}
            PipeState::Applier => return Err(E_BAD_STATE),
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

/// 自例程：R1 四类统一入口 → R2 边界 → R3 掩码外位注入 → R4 零变更守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 正常路径：chmod/chown/truncate/touch 四类各走一次统一管道。
    let mut p = SetattrPipe::new(0o644, 1000, 1000, 4096);
    let m = IAttr { mask: ATTR_MODE, mode: 0o600, uid: 0, gid: 0, size: 0, stamp_tick: 0 };
    p.apply(REQ_CHMOD, &m)?;
    let o = IAttr { mask: ATTR_UID | ATTR_GID, mode: 0, uid: 0, gid: 7, size: 0, stamp_tick: 0 };
    p.apply(REQ_CHOWN, &o)?;
    let t = IAttr { mask: ATTR_SIZE, mode: 0, uid: 0, gid: 0, size: 128, stamp_tick: 0 };
    p.apply(REQ_TRUNCATE, &t)?;
    let u = IAttr { mask: ATTR_ATIME | ATTR_MTIME, mode: 0, uid: 0, gid: 0, size: 0, stamp_tick: 55 };
    p.apply(REQ_TOUCH, &u)?;
    if p.inode.mode != 0o600 || p.inode.uid != 0 || p.inode.gid != 7 {
        return Err(E_ATTR_MIX);
    }
    if p.inode.size != 128 || p.inode.stamp_tick != 55 {
        return Err(E_ATTR_MIX);
    }
    p.writeback(true, 0x50)?;
    hits += 1;

    // R2 边界：掩码 0 / mode 超 0o7777 / truncate 回涨。
    let mut p = SetattrPipe::new(0o644, 1, 1, 10);
    let z = IAttr { mask: 0, mode: 0, uid: 0, gid: 0, size: 0, stamp_tick: 0 };
    if p.apply(REQ_CHMOD, &z).is_ok() {
        return Err(E_ZERO_MASK);
    }
    let big = IAttr { mask: ATTR_MODE, mode: 0o10000, uid: 0, gid: 0, size: 0, stamp_tick: 0 };
    if p.apply(REQ_CHMOD, &big).is_ok() {
        return Err(E_MODE_BITS);
    }
    let up = IAttr { mask: ATTR_SIZE, mode: 0, uid: 0, gid: 0, size: 11, stamp_tick: 0 };
    if p.apply(REQ_TRUNCATE, &up).is_ok() {
        return Err(E_SIZE_UP);
    }
    hits += 1;

    // R3 注入拦截：掩码外位（0x40 起）与组合错配全部被拒。
    let mut p = SetattrPipe::new(0o600, 1, 1, 8);
    let fp = p.fingerprint_pub();
    let evil = IAttr { mask: ATTR_MODE | 0x40, mode: 0o400, uid: 0, gid: 0, size: 0, stamp_tick: 0 };
    if p.apply(REQ_CHMOD, &evil).is_ok() {
        return Err(E_BAD_MASK);
    }
    p.verify_unchanged(fp)?; // 拒收必须零变更（守恒）
    let mixed = IAttr { mask: ATTR_MODE | ATTR_SIZE, mode: 0o400, uid: 0, gid: 0, size: 4, stamp_tick: 0 };
    if p.apply(REQ_CHMOD, &mixed).is_ok() {
        return Err(E_ATTR_MIX);
    }
    if p.rejected_total != 2 {
        return Err(E_BAD_STATE);
    }
    hits += 1;

    // R4 守恒对账：四类计数合计 = 落账数，审计过；truncate 边界恰等通过。
    let mut p = SetattrPipe::new(0o644, 1, 1, 100);
    let eq = IAttr { mask: ATTR_SIZE, mode: 0, uid: 0, gid: 0, size: 100, stamp_tick: 0 };
    p.apply(REQ_TRUNCATE, &eq)?; // 恰等尺寸允许（不涨即过）
    let m = IAttr { mask: ATTR_MODE, mode: 0o755, uid: 0, gid: 0, size: 0, stamp_tick: 0 };
    p.apply(REQ_CHMOD, &m)?;
    p.audit()?;
    hits += 1;

    // R5 追加回归：chown 大数 uid/gid 与 touch 同戳幂等落账。
    let mut p = SetattrPipe::new(0o644, 0, 0, 4);
    let big = IAttr { mask: ATTR_UID | ATTR_GID, mode: 0, uid: 0xFFFF_FFFF, gid: 0xFFFF_FFFF, size: 0, stamp_tick: 0 };
    p.apply(REQ_CHOWN, &big)?;
    let u1 = IAttr { mask: ATTR_ATIME | ATTR_MTIME, mode: 0, uid: 0, gid: 0, size: 0, stamp_tick: 777 };
    p.apply(REQ_TOUCH, &u1)?;
    let u2 = IAttr { mask: ATTR_ATIME | ATTR_MTIME, mode: 0, uid: 0, gid: 0, size: 0, stamp_tick: 777 };
    p.apply(REQ_TOUCH, &u2)?; // 同戳重复 touch 允许（幂等落账）
    if p.inode.uid != 0xFFFF_FFFF || p.inode.stamp_tick != 777 {
        return Err(E_ATTR_MIX);
    }
    if p.req_counts[REQ_TOUCH as usize] != 2 {
        return Err(E_BAL);
    }
    p.audit()?;
    hits += 1;

    Ok(hits)
}
