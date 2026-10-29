//! unx4052_setattr — UNX-F4052 setattr/chown/chmod 语义锚（诊断段 0x88E0）。
//!
//! 判据原文（提炼）：
//!   J1. chmod/chown/truncate 三入口统一经 setattr 管道落账，无一旁路。
//!   J2. 权限校验先行：无权注入返回 EPERM 且元数据逐字段不变。
//!   J3. 变更掩码精确落账：掩码未置位的字段绝不被改写。
//!   J4. 变更账守恒：管道受理数 == chmod+chown+truncate 入口合计。
//! 设计纪律：
//!   1. POSIX inode_change 语义：setattr 是唯一元数据写入口，
//!      chown/chmod/truncate 只是预组包的便捷入口，最终全部汇聚。
//!   2. 权限模型：属主或特权（uid 0）方可 setattr；非属主注入
//!      在校验闸门前拦截，账面零变动（判据 J2）。
//!   3. 变更掩码位定编：MODE/UID/GID/SIZE 四位，逐位审计落账范围。
//!   4. truncate 只缩不涨为合法，涨长注入按 EINVAL 拒收（本锚定编）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x88E0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ENOENT: u16 = SEG_BASE + 1;  // 目标 inode 不存在
const E_EPERM: u16 = SEG_BASE + 2;   // 非属主非特权，权限校验拒绝
const E_EINVAL: u16 = SEG_BASE + 3;  // 参数非法（如 truncate 涨长）
const E_NOCHANGE: u16 = SEG_BASE + 4;// 掩码为空（无字段可落账）
const E_LEDGER: u16 = SEG_BASE + 5;  // 变更账与入口账不平
const E_META_DRIFT: u16 = SEG_BASE + 6; // EPERM 后元数据漂移（判据失守）
const E_DUP_INO: u16 = SEG_BASE + 7; // 注册重复 inode 号

/// 变更掩码位定编（setattr 管道四字段，来源 POSIX setattr 语义）。
pub const SM_MODE: u16 = 0x0001; // 权限位变更
pub const SM_UID: u16 = 0x0002;  // 属主变更
pub const SM_GID: u16 = 0x0004;  // 属组变更
pub const SM_SIZE: u16 = 0x0008; // 长度截断变更

/// 特权 uid：0（POSIX root 语义）。
pub const PRIV_UID: u32 = 0;

/// inode 元数据实态（setattr 管道的写侧真值源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InodeMeta {
    pub ino: u64,  // inode 号
    pub mode: u32, // 权限位（低 12 位）+ 类型（高位）
    pub uid: u32,  // 属主
    pub gid: u32,  // 属组
    pub size: u64, // 字节数
    pub ctime: u64, // 元数据变更时刻（整数时钟刻）
}

/// setattr 请求：掩码 + 新值包（未置位字段的新值须为 0）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SetattrReq {
    pub ino: u64,   // 目标 inode
    pub mask: u16,  // SM_* 逐位或组合
    pub mode: u32,  // 新权限位（mask & SM_MODE 时生效）
    pub uid: u32,   // 新属主
    pub gid: u32,   // 新属组
    pub size: u64,  // 新长度（只允许缩小）
}

/// setattr 管道执行结果回执。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SetattrAck {
    pub ino: u64,    // 受理目标
    pub mask: u16,   // 实际落账掩码
    pub changed: u32, // 实际改变的字段数
}

/// setattr 状态机：请求从提交到落账的生命周期。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SaPhase {
    Submitted, // 已提交，未校验
    Checked,   // 权限/参数校验通过
    Applied,   // 已落账
    Rejected,  // 被拒（EPERM/EINVAL/ENOENT）
}

/// setattr 管道枢纽：元数据表 + 权限闸门 + 变更账。
pub struct SetattrPipe {
    pub tab: Vec<InodeMeta>,  // 元数据真值源
    pub caller_uid: u32,      // 当前调用者（闸门判据）
    pub calls_chmod: u32,     // chmod 入口计数
    pub calls_chown: u32,     // chown 入口计数
    pub calls_trunc: u32,     // truncate 入口计数
    pub applied_total: u32,   // 管道落账总数（守恒主账）
    pub rejected_total: u32,  // 拒收总数（含 EPERM）
    pub eperm_total: u32,     // EPERM 专项计数（判据 J2 面）
}

impl SetattrPipe {
    pub fn new(caller_uid: u32) -> SetattrPipe {
        SetattrPipe {
            tab: Vec::new(),
            caller_uid,
            calls_chmod: 0,
            calls_chown: 0,
            calls_trunc: 0,
            applied_total: 0,
            rejected_total: 0,
            eperm_total: 0,
        }
    }

    fn find(&self, ino: u64) -> Option<usize> {
        for i in 0..self.tab.len() {
            if self.tab[i].ino == ino {
                return Some(i);
            }
        }
        None
    }

    /// 注册元数据真值源（0 号与重复号拒收）。
    pub fn register(&mut self, m: InodeMeta) -> Result<(), u16> {
        if m.ino == 0 {
            return Err(E_ENOENT);
        }
        if self.find(m.ino).is_some() {
            return Err(E_DUP_INO);
        }
        self.tab.push(m);
        Ok(())
    }

    /// 权限闸门：属主或特权放行，其余 EPERM。
    fn gate(&mut self, m: &InodeMeta) -> Result<(), u16> {
        if self.caller_uid == m.uid || self.caller_uid == PRIV_UID {
            Ok(())
        } else {
            self.rejected_total += 1;
            self.eperm_total += 1;
            Err(E_EPERM)
        }
    }

    /// setattr 核心管道：权限校验先行 → 掩码逐位落账。
    /// 返回执行回执；EPERM 时元数据逐字段不变（判据 J2）。
    pub fn setattr(&mut self, req: SetattrReq) -> Result<SetattrAck, SetattrErr> {
        self.setattr_impl(req).map_err(|e| SetattrErr { code: e })
    }

    fn setattr_impl(&mut self, req: SetattrReq) -> Result<SetattrAck, u16> {
        let i = match self.find(req.ino) {
            Some(i) => i,
            None => {
                self.rejected_total += 1;
                return Err(E_ENOENT);
            }
        };
        if req.mask == 0 {
            self.rejected_total += 1;
            return Err(E_NOCHANGE);
        }
        // 权限校验先行（判据 J2：先闸门后写账）
        let owner = self.tab[i].uid;
        if !(self.caller_uid == owner || self.caller_uid == PRIV_UID) {
            self.rejected_total += 1;
            self.eperm_total += 1;
            return Err(E_EPERM);
        }
        // 参数校验：truncate 只缩不涨
        if req.mask & SM_SIZE != 0 && req.size > self.tab[i].size {
            self.rejected_total += 1;
            return Err(E_EINVAL);
        }
        let mut changed: u32 = 0;
        if req.mask & SM_MODE != 0 {
            self.tab[i].mode = req.mode;
            changed += 1;
        }
        if req.mask & SM_UID != 0 {
            self.tab[i].uid = req.uid;
            changed += 1;
        }
        if req.mask & SM_GID != 0 {
            self.tab[i].gid = req.gid;
            changed += 1;
        }
        if req.mask & SM_SIZE != 0 {
            self.tab[i].size = req.size;
            self.tab[i].ctime += 1; // 元数据变更时刻推进（整数刻）
            changed += 1;
        }
        if req.mask & (SM_MODE | SM_UID | SM_GID) != 0 {
            self.tab[i].ctime += 1;
        }
        self.applied_total += 1;
        Ok(SetattrAck { ino: req.ino, mask: req.mask, changed })
    }

    /// chmod 便捷入口：预组包后统一走 setattr 管道（判据 J1）。
    pub fn chmod(&mut self, ino: u64, mode: u32) -> Result<SetattrAck, SetattrErr> {
        self.calls_chmod += 1;
        self.setattr(SetattrReq { ino, mask: SM_MODE, mode, uid: 0, gid: 0, size: 0 })
    }

    /// chown 便捷入口：属主/属组统一组包落管道。
    pub fn chown(&mut self, ino: u64, uid: u32, gid: u32) -> Result<SetattrAck, SetattrErr> {
        self.calls_chown += 1;
        let mut mask: u16 = 0;
        if uid != 0 { mask |= SM_UID; }
        if gid != 0 { mask |= SM_GID; }
        self.setattr(SetattrReq { ino, mask, mode: 0, uid, gid, size: 0 })
    }

    /// truncate 便捷入口：只缩不涨，涨长按 EINVAL 拒收。
    pub fn truncate(&mut self, ino: u64, size: u64) -> Result<SetattrAck, SetattrErr> {
        self.calls_trunc += 1;
        self.setattr(SetattrReq { ino, mask: SM_SIZE, mode: 0, uid: 0, gid: 0, size })
    }

    /// EPERM 注入后元数据逐字段复核（判据 J2 的对账面）。
    pub fn verify_untouched(&self, ino: u64, snap: InodeMeta) -> Result<(), u16> {
        match self.find(ino) {
            Some(i) => {
                if self.tab[i] == snap {
                    Ok(())
                } else {
                    Err(E_META_DRIFT)
                }
            }
            None => Err(E_ENOENT),
        }
    }

    /// 元数据快照（EPERM 注入前留存用）。
    pub fn snapshot(&self, ino: u64) -> Result<InodeMeta, u16> {
        match self.find(ino) {
            Some(i) => Ok(self.tab[i]),
            None => Err(E_ENOENT),
        }
    }

    /// 守恒对账：三入口合计 == 落账 + 拒收。
    pub fn ledger_ok(&self) -> bool {
        let entries = self.calls_chmod + self.calls_chown + self.calls_trunc;
        entries == self.applied_total + self.rejected_total
    }
}

/// 便捷入口的错误包装（统一 Result 语义层）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SetattrErr {
    pub code: u16,
}

impl From<SetattrErr> for u16 {
    fn from(e: SetattrErr) -> u16 {
        e.code
    }
}

/// 自例程：三入口汇聚→权限注入→掩码精确性→守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut p = SetattrPipe::new(1000); // 调用者 uid 1000

    // R1 三入口汇聚：chmod/chown/truncate 全部落 setattr 管道
    let meta = InodeMeta { ino: 5, mode: 0o100_644, uid: 1000, gid: 1000, size: 8192, ctime: 0 };
    p.register(meta)?;
    let a1 = p.chmod(5, 0o100_600)?;
    let a2 = p.chown(5, 1001, 1001)?;
    let a3 = p.truncate(5, 4096)?;
    if a1.mask != SM_MODE || a2.mask != (SM_UID | SM_GID) || a3.mask != SM_SIZE {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R2 权限注入：非属主 chmod → EPERM 且元数据快照逐字段不变
    let snap = p.snapshot(5)?;
    let mut q = SetattrPipe::new(7777);
    q.tab.push(snap);
    match q.chmod(5, 0o777) {
        Err(e) if e.code == E_EPERM => {}
        _ => return Err(E_EPERM),
    }
    q.verify_untouched(5, snap)?;
    hits += 1;

    // R3 掩码精确性：truncate 涨长拒收（EINVAL），掩码为空拒收
    match p.truncate(5, 99999) {
        Err(e) if e.code == E_EINVAL => {}
        _ => return Err(E_EINVAL),
    }
    match p.setattr(SetattrReq { ino: 5, mask: 0, mode: 0, uid: 0, gid: 0, size: 0 }) {
        Err(e) if e.code == E_NOCHANGE => {}
        _ => return Err(E_NOCHANGE),
    }
    hits += 1;

    // R4 守恒对账：三入口合计 == 落账 + 拒收
    if !p.ledger_ok() {
        return Err(E_LEDGER);
    }
    if p.calls_chmod != 1 || p.calls_chown != 1 || p.calls_trunc != 2 {
        return Err(E_LEDGER);
    }
    if p.applied_total != 3 || p.rejected_total != 3 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R5 特权通道：uid 0 越权改属主，EPERM 面回归零漂移
    let mut r = SetattrPipe::new(PRIV_UID);
    let m2 = InodeMeta { ino: 9, mode: 0o100_644, uid: 42, gid: 42, size: 16, ctime: 0 };
    r.register(m2)?;
    r.chown(9, 1000, 1000)?;
    let m3 = r.snapshot(9)?;
    if m3.uid != 1000 || m3.gid != 1000 {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
