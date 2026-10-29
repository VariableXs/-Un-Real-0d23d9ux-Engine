//! unx4005_ugidentity — UNX-F4005 uid/gid 属主模型与凭据传递接口（诊断段 0x8360）。
//!
//! 判据原文：
//!   J1 属主三元组（uid/gid/mode）在 inode 创建链三段（syscal 层 → VFS 层
//!      → fs 嫁接层）传递保真，每段落账可查。
//!   J2 凭据缺省值（uid=0/gid=0）落账：未显式给凭据时创建链以 ROOT 凭据
//!      补全，缺省事件入账。
//!   J3 传递链中途抹除注入（任一段凭据清零篡改）在第三段被检出。
//!   J4 三段凭据快照逐一比对一致才算保真账成立，任何一段不一致即断链。
//! 设计纪律：
//!   1. 凭据对象 Cred { uid: u32, gid: u32 } 与属主对象 Owner
//!      { uid, gid, mode } 分离：凭据是请求者身份，属主是 inode 落地值。
//!   2. 创建链三段状态机：Syscall → Vfs → FsGraft，单向推进不可回跳；
//!      每段移交时对凭据做快照比对，快照不一致立刻断链报错。
//!   3. 抹除检测：u32 全零凭据仅在「显式缺省补全」分支合法；链中途出现
//!      全零即视为抹除注入，第三段终审拒绝。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8360;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_CHAIN_BAD: u16 = SEG_BASE + 1;  // 创建链段序非法（回跳/越段）
const E_CRED_WIPE: u16 = SEG_BASE + 2;  // 链中途凭据被抹除（全零注入）
const E_SNAP_DIFF: u16 = SEG_BASE + 3;  // 三段快照不一致（保真失守）
const E_OWNER_DIFF: u16 = SEG_BASE + 4; // 第三段属主落地与凭据不符
const E_BAL: u16 = SEG_BASE + 5;        // 传递账与总请求账不平
const E_MODE_OVER: u16 = SEG_BASE + 6;  // mode 权限域越界（高于 0o7777）

/// 权限域掩码（与权限位模型共用十二位域定义）。
pub const PERM_MASK: u32 = 0o7777;

/// 请求者凭据：uid/gid 对。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cred {
    pub uid: u32, // 用户号
    pub gid: u32, // 组号
}

impl Cred {
    /// 缺省凭据：0/0（ROOT），判据 J2 缺省补全分支的落账值。
    pub fn default_cred() -> Cred {
        Cred { uid: 0, gid: 0 }
    }

    /// 全零判定：链中途出现即视为抹除注入（判据 J3）。
    pub fn is_wiped(&self) -> bool {
        self.uid == 0 && self.gid == 0
    }
}

/// inode 属主三元组落地值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Owner {
    pub uid: u32,  // 属主 uid
    pub gid: u32,  // 属主 gid
    pub mode: u32, // 十二位权限域
}

impl Owner {
    pub fn from_cred(c: Cred, mode: u32) -> Result<Owner, u16> {
        if mode & !PERM_MASK != 0 {
            return Err(E_MODE_OVER);
        }
        Ok(Owner { uid: c.uid, gid: c.gid, mode })
    }
}

/// 创建链三段状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainStage {
    Syscall = 0, // 系统调用层：凭据入口
    Vfs = 1,     // VFS 层：凭据中转
    FsGraft = 2, // fs 嫁接层：属主落地终审
}

/// 单次创建链的账目单元：凭据 + 三段快照 + 落地属主。
pub struct ChainRecord {
    pub cred: Cred,               // 入口凭据
    pub snap: [Cred; 3],          // 三段移交时快照
    pub stage_done: u32,          // 已推进段数（0..3）
    pub owner: Option<Owner>,     // 第三段落地属主
    pub default_used: bool,       // 是否走缺省补全分支
    pub detected_wipe: bool,      // 是否检出抹除注入
}

impl ChainRecord {
    fn new(cred: Cred) -> ChainRecord {
        ChainRecord {
            cred,
            snap: [cred, cred, cred],
            stage_done: 0,
            owner: None,
            default_used: false,
            detected_wipe: false,
        }
    }
}

/// 属主模型基座：创建链账 + 三账计数。
pub struct UgidBase {
    pub records: Vec<ChainRecord>,       // 创建链记录账
    pub pass_count: u32,                 // 三段保真通过账
    pub wipe_count: u32,                 // 抹除检出账
    pub default_count: u32,              // 缺省补全账
    pub req_total: u32,                  // 总请求账
}

impl UgidBase {
    pub fn new() -> UgidBase {
        UgidBase {
            records: Vec::new(),
            pass_count: 0,
            wipe_count: 0,
            default_count: 0,
            req_total: 0,
        }
    }

    /// 发起创建链：None 凭据走缺省补全（0/0）并落缺省账。
    pub fn begin(&mut self, cred: Option<Cred>) -> usize {
        self.req_total += 1;
        let (c, dflt) = match cred {
            Some(c) => (c, false),
            None => (Cred::default_cred(), true),
        };
        if dflt {
            self.default_count += 1;
        }
        let mut r = ChainRecord::new(c);
        r.default_used = dflt; // 缺省补全链标记：全零凭据合法
        self.records.push(r);
        self.records.len() - 1
    }

    /// 推进一段：段序必须连续（0→1→2），中途移交做抹除检测与快照。
    pub fn advance(&mut self, idx: usize, cred: Cred) -> Result<ChainStage, u16> {
        if idx >= self.records.len() {
            return Err(E_CHAIN_BAD);
        }
        let done = self.records[idx].stage_done;
        if done >= 3 {
            return Err(E_CHAIN_BAD); // 已终审不可再推进
        }
        // 抹除注入检测：非首段出现全零凭据即断链（判据 J3）。
        // 缺省补全链（default_used）全零是合法值，不参与抹除判定。
        if done > 0 && cred.is_wiped() && !self.records[idx].default_used {
            self.records[idx].detected_wipe = true;
            self.wipe_count += 1;
            return Err(E_CRED_WIPE);
        }
        self.records[idx].snap[done as usize] = cred;
        self.records[idx].stage_done = done + 1;
        Ok(match done {
            0 => ChainStage::Vfs,
            1 => ChainStage::FsGraft,
            _ => ChainStage::Syscall, // 不可达，闭式占位
        })
    }

    /// 第三段终审：三段快照逐一比对 + 属主落地与凭据一致性。
    pub fn finalize(&mut self, idx: usize, mode: u32) -> Result<Owner, u16> {
        if idx >= self.records.len() {
            return Err(E_CHAIN_BAD);
        }
        let done = self.records[idx].stage_done;
        if done != 3 {
            return Err(E_CHAIN_BAD); // 三段未走完不得终审
        }
        let c0 = self.records[idx].snap[0];
        let c1 = self.records[idx].snap[1];
        let c2 = self.records[idx].snap[2];
        if c0 != c1 || c1 != c2 {
            return Err(E_SNAP_DIFF);
        }
        let o = Owner::from_cred(c2, mode)?;
        if o.uid != self.records[idx].cred.uid || o.gid != self.records[idx].cred.gid {
            return Err(E_OWNER_DIFF);
        }
        self.records[idx].owner = Some(o);
        self.pass_count += 1;
        Ok(o)
    }

    /// 快照导出：某记录的三段快照副本。
    pub fn snaps(&self, idx: usize) -> Result<[Cred; 3], u16> {
        if idx >= self.records.len() {
            return Err(E_CHAIN_BAD);
        }
        Ok(self.records[idx].snap)
    }

    /// 缺省补全账查询（判据 J2 读面）。
    pub fn default_used_count(&self) -> u32 {
        self.default_count
    }

    /// 守恒：保真账 + 抹除账 + 未完链账 == 总请求账。
    pub fn conserve(&self) -> bool {
        let mut unfinished = 0u32;
        for r in &self.records {
            if r.owner.is_none() && !r.detected_wipe {
                unfinished += 1;
            }
        }
        self.pass_count + self.wipe_count + unfinished == self.req_total
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_CHAIN_BAD => "创建链段序非法（回跳/越段/未走完终审）",
        E_CRED_WIPE => "链中途凭据被抹除（全零注入）",
        E_SNAP_DIFF => "三段快照不一致（保真失守）",
        E_OWNER_DIFF => "第三段属主落地与入口凭据不符",
        E_BAL => "传递账与总请求账不平",
        E_MODE_OVER => "mode 权限域越界（高于 0o7777）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：三段保真 → 缺省补全 → 抹除注入检出 → 守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut base = UgidBase::new();

    // R1：显式凭据 (1000,100) 三段推进保真落地（判据 J1 原文）。
    let i0 = base.begin(Some(Cred { uid: 1000, gid: 100 }));
    base.advance(i0, Cred { uid: 1000, gid: 100 })?;
    base.advance(i0, Cred { uid: 1000, gid: 100 })?;
    base.advance(i0, Cred { uid: 1000, gid: 100 })?;
    let o0 = base.finalize(i0, 0o644)?;
    if o0.uid != 1000 || o0.gid != 100 || o0.mode != 0o644 {
        return Err(E_OWNER_DIFF);
    }
    hits += 1;

    // R2：三段快照逐一可导且一致（判据 J4）。
    let s = base.snaps(i0)?;
    if s[0] != s[1] || s[1] != s[2] {
        return Err(E_SNAP_DIFF);
    }
    hits += 1;

    // R3：缺省凭据补全——None 入口落账 (0/0)，缺省账 +1（判据 J2）。
    let i1 = base.begin(None);
    base.advance(i1, Cred::default_cred())?;
    base.advance(i1, Cred::default_cred())?;
    base.advance(i1, Cred::default_cred())?;
    let o1 = base.finalize(i1, 0o755)?;
    if o1.uid != 0 || o1.gid != 0 {
        return Err(E_OWNER_DIFF);
    }
    if base.default_used_count() != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：三链并发在账——两条记录均已终审，通过账 = 2。
    if base.pass_count != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：第二次缺省补全（再验缺省分支可重复）。
    let i2 = base.begin(None);
    base.advance(i2, Cred::default_cred())?;
    base.advance(i2, Cred::default_cred())?;
    base.advance(i2, Cred::default_cred())?;
    base.finalize(i2, 0o600)?;
    if base.default_used_count() != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：链中途抹除注入——VFS 段给全零凭据，第三段前即被检出（判据 J3）。
    let i3 = base.begin(Some(Cred { uid: 2000, gid: 200 }));
    base.advance(i3, Cred { uid: 2000, gid: 200 })?;
    if base.advance(i3, Cred { uid: 0, gid: 0 }) != Err(E_CRED_WIPE) {
        return Err(E_CRED_WIPE);
    }
    if base.wipe_count != 1 {
        return Err(E_BAL);
    }
    // 被抹除链不得终审。
    if base.finalize(i3, 0o644) != Err(E_CHAIN_BAD) {
        return Err(E_CHAIN_BAD);
    }
    hits += 1;

    // I2：段序回跳注入——未走完三段就终审被拒。
    let i4 = base.begin(Some(Cred { uid: 3000, gid: 300 }));
    base.advance(i4, Cred { uid: 3000, gid: 300 })?;
    if base.finalize(i4, 0o644) != Err(E_CHAIN_BAD) {
        return Err(E_CHAIN_BAD);
    }
    hits += 1;

    // I3：mode 越界注入被拒。
    let i5 = base.begin(Some(Cred { uid: 1, gid: 1 }));
    base.advance(i5, Cred { uid: 1, gid: 1 })?;
    base.advance(i5, Cred { uid: 1, gid: 1 })?;
    base.advance(i5, Cred { uid: 1, gid: 1 })?;
    if base.finalize(i5, 0o17777) != Err(E_MODE_OVER) {
        return Err(E_MODE_OVER);
    }
    hits += 1;

    // R6：守恒对账——通过 3 + 抹除 1 + 未完 2 == 请求 6。
    if !base.conserve() {
        return Err(E_BAL);
    }
    if base.req_total != 6 || base.pass_count != 3 {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
