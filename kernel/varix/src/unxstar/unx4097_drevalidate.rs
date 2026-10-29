//! unx4097_drevalidate — UNX-F4097 d_revalidate 钩子锚（诊断段 0x8E40）。
//!
//! 判据原文：UNX-F4097-J1 钩子返回失效触发重建（重建计数=1），未实现钩子默认信任
//!           （可切换账），钩子异常返回被捕获不崩。
//! 可验性质（本模块自证面）：
//!   J1 钩子判 Stale 时，一次 revalidate 调用必须且只触发 1 次重建
//!      （rebuilds 增量 == 1），重建后条目 valid 且 gen +1；
//!   J2 钩子未装（Absent）时默认信任：返回 Trust 且 rebuilds 不增，
//!      该次信任计入 default_trusts 账（可切换账的"未实现"面）；
//!   J3 钩子异常（Err）必须被捕获：不 panic、不返回 Err 给上层，而是记 captured+1、
//!      按 Stale 处理（保守重建），返回值仍是 Ok(HookVerdict::HookErr)；
//!   J4 账守恒：revalidates == trusts + stales + captured（一路一账）；
//!      重建数 == stale 数 + captured 数（只有失效才重建）；
//!   J5 非法输入拒：不存在的 dentry（E_NO_DENTRY）、0 号 id（E_ID_ZERO）、
//!      非 Debug 场景下的未知钩子码（E_HOOK_OOR）——拒收不动任何账。
//! 设计纪律：
//!   1. 钩子是"可选能力"：Absent 不是错误，而是 VFS 里最常见的形态（多数 fs 不实现
//!      d_revalidate），所以默认信任必须是一等路径，且要单独记账以便审计。
//!   2. 异常一律吞掉转保守重建：钩子写坏了不能拖垮 lookup，只能让缓存多做一次重建；
//!      这条路必须有 captured 账，否则"永远重建"会被误读成"命中率差"。
//!   3. 重建只改本条 dentry（valid/gen/reborn），不动邻居、不动父链：重建是局部的。
//!   4. 钩子模式可切（Absent/AlwaysValid/GenMatch/Failing），切模式不清账，
//!      历史调用账保留（审计需要）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!
//! 诊断段 0x8E40 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8E40;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NO_DENTRY: u16 = SEG_BASE + 1; // dentry 不存在
const E_ID_ZERO: u16 = SEG_BASE + 2; // id=0 保留
const E_HOOK_OOR: u16 = SEG_BASE + 3; // 钩子码越界
const E_DUP_ID: u16 = SEG_BASE + 4; // dentry 重复建项
const E_BAL: u16 = SEG_BASE + 5; // 账不平
const E_STALE_CHAIN: u16 = SEG_BASE + 6; // 连续失效超限（钩子抖振）
const E_INO_ZERO: u16 = SEG_BASE + 7; // inode 号 0 保留
const E_NOT_VALID: u16 = SEG_BASE + 8; // 对无效条目做"仅信任"操作

/// 连续失效上限：同一条目连续被钩子判失效超过此次数即判钩子抖振。
pub const STALE_LIMIT: u32 = 16;

/// 钩子种类闭集（Absent 是一等形态，不是 None 值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HookKind {
    Absent = 0,      // 未实现 d_revalidate → 默认信任
    AlwaysValid = 1, // 恒判有效
    GenMatch = 2,    // 比对代次，不一致即失效
    Failing = 3,     // 恒返异常（模拟钩子写坏）
}

/// 钩子裁决：三态 + 异常携带码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HookVerdict {
    Trust = 0,       // 信任缓存
    Stale = 1,       // 失效，需重建
    HookErr = 2,     // 钩子异常（已捕获，按失效处理）
}

/// 钩子本体：种类 + 调用账。
pub struct RevalHook {
    pub kind: HookKind,    // 当前钩子种类
    pub calls: u32,        // 被调用次数
    pub errors: u32,       // 返回异常次数
    pub switches: u32,     // 种类切换次数
    pub err_code: u16,     // 异常码（Failing 时返回它）
}

impl RevalHook {
    pub fn new(kind: HookKind) -> RevalHook {
        RevalHook {
            kind,
            calls: 0,
            errors: 0,
            switches: 0,
            err_code: SEG_BASE + 0x40, // 钩子自定义异常码（段内偏移，不与诊断码冲突）
        }
    }

    /// 钩子码 → 闭集。
    pub fn kind_by_code(code: u16) -> Result<HookKind, u16> {
        match code {
            0 => Ok(HookKind::Absent),
            1 => Ok(HookKind::AlwaysValid),
            2 => Ok(HookKind::GenMatch),
            3 => Ok(HookKind::Failing),
            _ => Err(E_HOOK_OOR),
        }
    }

    /// 切种类（幂等：同种类不计切换）。
    pub fn set_kind(&mut self, k: HookKind) -> Result<(), u16> {
        if self.kind != k {
            self.kind = k;
            self.switches += 1;
        }
        Ok(())
    }

    /// 调钩子：输入 (缓存代次, fs 代次)，输出裁决。Absent 直接 Trust。
    pub fn invoke(&mut self, cached_gen: u32, fs_gen: u32) -> HookVerdict {
        self.calls += 1;
        match self.kind {
            HookKind::Absent => HookVerdict::Trust,
            HookKind::AlwaysValid => HookVerdict::Trust,
            HookKind::GenMatch => {
                if cached_gen == fs_gen {
                    HookVerdict::Trust
                } else {
                    HookVerdict::Stale
                }
            }
            HookKind::Failing => {
                self.errors += 1;
                HookVerdict::HookErr
            }
        }
    }
}

/// dentry 缓存条目：id + ino + 代次 + 有效位。
pub struct DEntryRec {
    pub id: u64,         // dentry 标识
    pub ino: u64,        // 绑定 inode
    pub gen: u32,        // 缓存代次（重建 +1）
    pub valid: bool,     // 有效位
    pub reborn: u32,     // 本条被重建次数
    pub stale_run: u32,  // 连续被判失效次数（抖振检测）
}

/// 重建统计账。
pub struct RevalStat {
    pub revalidates: u32,   // revalidate 调用次数（成功入账）
    pub trusts: u32,        // 判信任次数
    pub stales: u32,        // 判失效次数
    pub captured: u32,      // 异常捕获次数（按失效处理）
    pub default_trusts: u32, // 未装钩子的默认信任次数
    pub rebuilds: u32,      // 重建次数
    pub rejects: u32,       // 拒收次数
}

impl RevalStat {
    fn new() -> RevalStat {
        RevalStat {
            revalidates: 0,
            trusts: 0,
            stales: 0,
            captured: 0,
            default_trusts: 0,
            rebuilds: 0,
            rejects: 0,
        }
    }

    /// J4 其一：一路一账。
    pub fn walks_conserved(&self) -> bool {
        self.revalidates == self.trusts + self.stales + self.captured
    }

    /// J4 其二：只有失效（含异常）才重建。
    pub fn rebuild_conserved(&self) -> bool {
        self.rebuilds == self.stales + self.captured
    }
}

/// d_revalidate 枢纽：条目表 + 钩子 + 账 + 外部 fs 代次表。
pub struct DCacheReval {
    pub ents: Vec<DEntryRec>, // dentry 条目
    pub fs_gen: Vec<(u64, u32)>, // fs 侧代次表（ino → 代次）
    pub hook: RevalHook,      // 钩子
    pub stat: RevalStat,      // 账
}

impl DCacheReval {
    pub fn new(kind: HookKind) -> DCacheReval {
        DCacheReval {
            ents: Vec::new(),
            fs_gen: Vec::new(),
            hook: RevalHook::new(kind),
            stat: RevalStat::new(),
        }
    }

    /// 建条目：id 非 0 且唯一、ino 非 0。
    pub fn add(&mut self, id: u64, ino: u64) -> Result<(), u16> {
        if id == 0 {
            return Err(E_ID_ZERO);
        }
        if ino == 0 {
            return Err(E_INO_ZERO);
        }
        if self.find(id).is_some() {
            return Err(E_DUP_ID);
        }
        self.ents.push(DEntryRec {
            id,
            ino,
            gen: 1,
            valid: true,
            reborn: 0,
            stale_run: 0,
        });
        Ok(())
    }

    /// 条目定位。
    pub fn find(&self, id: u64) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ents.len() {
            if self.ents[i].id == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// fs 侧代次登记/推进（模拟 fs 侧换 inode）。
    pub fn fs_bump(&mut self, ino: u64) -> Result<u32, u16> {
        let mut i = 0usize;
        while i < self.fs_gen.len() {
            if self.fs_gen[i].0 == ino {
                self.fs_gen[i].1 += 1;
                return Ok(self.fs_gen[i].1);
            }
            i += 1;
        }
        self.fs_gen.push((ino, 2));
        Ok(2)
    }

    /// fs 侧代次读数（未登记视为 1，与新建条目对齐）。
    pub fn fs_gen_of(&self, ino: u64) -> u32 {
        let mut i = 0usize;
        while i < self.fs_gen.len() {
            if self.fs_gen[i].0 == ino {
                return self.fs_gen[i].1;
            }
            i += 1;
        }
        1
    }

    /// 重建：置 valid、gen 换到 fs 侧当前代次、reborn +1、rebuilds +1。
    fn rebuild(&mut self, idx: usize) {
        let ino = self.ents[idx].ino;
        let g = self.fs_gen_of(ino);
        self.ents[idx].gen = g;
        self.ents[idx].valid = true;
        self.ents[idx].reborn += 1;
        self.stat.rebuilds += 1;
    }

    /// 钩子锚（J1/J2/J3 唯一入口）。
    pub fn revalidate(&mut self, id: u64) -> Result<HookVerdict, u16> {
        if id == 0 {
            self.stat.rejects += 1;
            return Err(E_ID_ZERO);
        }
        let idx = match self.find(id) {
            Some(i) => i,
            None => {
                self.stat.rejects += 1;
                return Err(E_NO_DENTRY);
            }
        };
        let cached_gen = self.ents[idx].gen;
        let fs_gen = self.fs_gen_of(self.ents[idx].ino);
        let v = self.hook.invoke(cached_gen, fs_gen);
        // 抖振防护前置：超限即拒，且不落取数账、不落失效账（否则重建账会被带偏）
        if let HookVerdict::Stale = v {
            if self.ents[idx].stale_run + 1 > STALE_LIMIT {
                self.stat.rejects += 1;
                return Err(E_STALE_CHAIN);
            }
        }
        self.stat.revalidates += 1;
        match v {
            HookVerdict::Trust => {
                self.stat.trusts += 1;
                if self.hook.kind == HookKind::Absent {
                    self.stat.default_trusts += 1; // 未实现钩子的默认信任单列
                }
                self.ents[idx].stale_run = 0;
                Ok(HookVerdict::Trust)
            }
            HookVerdict::Stale => {
                self.stat.stales += 1;
                self.ents[idx].stale_run += 1;
                self.ents[idx].valid = false;
                self.rebuild(idx); // 恰好一次重建
                Ok(HookVerdict::Stale)
            }
            HookVerdict::HookErr => {
                // 异常捕获：不 panic、不上抛，保守按失效处理（设计纪律 2）
                self.stat.captured += 1;
                self.ents[idx].valid = false;
                self.rebuild(idx);
                Ok(HookVerdict::HookErr)
            }
        }
    }

    /// 仅信任口：钩子判信任后上层可直接用它（对无效条目调用必须拒）。
    pub fn trust_only(&mut self, id: u64) -> Result<u64, u16> {
        let idx = match self.find(id) {
            Some(i) => i,
            None => return Err(E_NO_DENTRY),
        };
        if !self.ents[idx].valid {
            return Err(E_NOT_VALID);
        }
        Ok(self.ents[idx].ino)
    }

    /// 账终审：两路守恒 + 默认信任不超过总信任。
    pub fn audit(&self) -> bool {
        self.stat.walks_conserved()
            && self.stat.rebuild_conserved()
            && self.stat.default_trusts <= self.stat.trusts
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NO_DENTRY => "dentry 不存在",
        E_ID_ZERO => "dentry id=0 保留",
        E_HOOK_OOR => "钩子码越界（合法 0..3）",
        E_DUP_ID => "dentry 重复建项",
        E_BAL => "revalidate/重建账不平",
        E_STALE_CHAIN => "连续失效超限（钩子抖振）",
        E_INO_ZERO => "inode 号 0 保留",
        E_NOT_VALID => "对无效条目做仅信任操作",
        _ => "未知诊断码",
    }
}

/// 确定性演练：默认信任 → 失效重建 → 异常捕获 → 模式切换 → 注入拦截。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：钩子未装（Absent）——10 次调用全 Trust，default_trusts == 10，零重建（J2）。
    let mut d = DCacheReval::new(HookKind::Absent);
    let mut i = 0u64;
    while i < 10 {
        d.add(0x7000 + i, 0x9000 + i)?;
        i += 1;
    }
    let mut k = 0u64;
    while k < 10 {
        if d.revalidate(0x7000 + k)? != HookVerdict::Trust {
            return Err(E_BAL);
        }
        k += 1;
    }
    if d.stat.trusts != 10 || d.stat.default_trusts != 10 {
        return Err(E_BAL);
    }
    if d.stat.rebuilds != 0 {
        return Err(E_BAL); // 默认信任绝不重建
    }
    if !d.audit() {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：切到 GenMatch，fs 侧推进代次 → 条目失效并恰好重建 1 次（J1 主面）。
    d.hook.set_kind(HookKind::GenMatch)?;
    if d.hook.switches != 1 {
        return Err(E_BAL);
    }
    d.fs_bump(0x9003)?; // fs 侧 3 号换了 inode
    let rb_before = d.stat.rebuilds;
    if d.revalidate(0x7003)? != HookVerdict::Stale {
        return Err(E_BAL);
    }
    if d.stat.rebuilds != rb_before + 1 {
        return Err(E_BAL); // 重建计数必须恰为 1
    }
    if d.ents[d.find(0x7003).ok_or(E_NO_DENTRY)?].reborn != 1 {
        return Err(E_BAL);
    }
    if !d.ents[d.find(0x7003).ok_or(E_NO_DENTRY)?].valid {
        return Err(E_BAL); // 重建后必须有效
    }
    if d.ents[d.find(0x7003).ok_or(E_NO_DENTRY)?].gen != d.fs_gen_of(0x9003) {
        return Err(E_BAL); // 代次换到 fs 侧当前值
    }
    hits += 1;

    // R3：代次已对齐后再调 → Trust（重建后不再抖振）。
    let rb2 = d.stat.rebuilds;
    if d.revalidate(0x7003)? != HookVerdict::Trust {
        return Err(E_BAL);
    }
    if d.stat.rebuilds != rb2 {
        return Err(E_BAL);
    }
    if d.ents[d.find(0x7003).ok_or(E_NO_DENTRY)?].stale_run != 0 {
        return Err(E_BAL); // 信任清零连续失效计数
    }
    hits += 1;

    // R4：钩子写坏（Failing）——异常被捕获、按失效处理、重建 1 次，不 panic（J3）。
    d.hook.set_kind(HookKind::Failing)?;
    let cap_before = d.stat.captured;
    let rb3 = d.stat.rebuilds;
    if d.revalidate(0x7005)? != HookVerdict::HookErr {
        return Err(E_BAL); // 异常被吞成裁决，不向上抛 Err
    }
    if d.stat.captured != cap_before + 1 {
        return Err(E_BAL);
    }
    if d.stat.rebuilds != rb3 + 1 {
        return Err(E_BAL); // 异常也走一次保守重建
    }
    if d.hook.errors != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：连 10 次失效（GenMatch + 每轮 fs 推进）——每轮重建 1 次，stale_run 递增。
    d.hook.set_kind(HookKind::GenMatch)?;
    let mut m = 0u32;
    while m < 10 {
        d.fs_bump(0x9007)?;
        if d.revalidate(0x7007)? != HookVerdict::Stale {
            return Err(E_BAL);
        }
        m += 1;
    }
    let idx7 = d.find(0x7007).ok_or(E_NO_DENTRY)?;
    if d.ents[idx7].reborn != 10 {
        return Err(E_BAL); // 10 轮 10 次重建
    }
    if d.ents[idx7].stale_run != 10 {
        return Err(E_BAL);
    }
    hits += 1;

    // R6：抖振防护——连续失效超过 STALE_LIMIT 后拒（E_STALE_CHAIN）。
    let mut n = 0u32;
    while n < STALE_LIMIT {
        d.fs_bump(0x9001)?;
        if d.revalidate(0x7001)? != HookVerdict::Stale {
            return Err(E_BAL);
        }
        n += 1;
    }
    d.fs_bump(0x9001)?;
    match d.revalidate(0x7001) {
        Err(E_STALE_CHAIN) => {}
        _ => return Err(E_STALE_CHAIN),
    }
    hits += 1;

    // I1：不存在的 dentry / 0 号 → 拒，且 revalidates 不增（拒收先行）。
    let rv_before = d.stat.revalidates;
    if d.revalidate(0xDEAD) != Err(E_NO_DENTRY) {
        return Err(E_NO_DENTRY);
    }
    if d.revalidate(0) != Err(E_ID_ZERO) {
        return Err(E_ID_ZERO);
    }
    if d.stat.revalidates != rv_before {
        return Err(E_BAL);
    }
    hits += 1;

    // I2：钩子码越界（4 / 65535）→ E_HOOK_OOR。
    if RevalHook::kind_by_code(4) != Err(E_HOOK_OOR) {
        return Err(E_HOOK_OOR);
    }
    if RevalHook::kind_by_code(0xFFFF) != Err(E_HOOK_OOR) {
        return Err(E_HOOK_OOR);
    }
    hits += 1;

    // I3：重复建项 / 0 号 id / 0 号 ino 三路拒。
    if d.add(0x7000, 1) != Err(E_DUP_ID) {
        return Err(E_DUP_ID);
    }
    if d.add(0, 1) != Err(E_ID_ZERO) {
        return Err(E_ID_ZERO);
    }
    if d.add(0x7777, 0) != Err(E_INO_ZERO) {
        return Err(E_INO_ZERO);
    }
    hits += 1;

    // I4：对无效条目做 trust_only → E_NOT_VALID（防"未重建就拿去用"）。
    let mut e = DCacheReval::new(HookKind::Failing);
    e.add(0x7100, 0x9100)?;
    // Failing 钩子：revalidate 后条目仍会被重建为有效，故这里手工置无效再测
    e.ents[0].valid = false;
    if e.trust_only(0x7100) != Err(E_NOT_VALID) {
        return Err(E_NOT_VALID);
    }
    e.ents[0].valid = true;
    if e.trust_only(0x7100)? != 0x9100 {
        return Err(E_BAL);
    }
    hits += 1;

    // 终审：两路守恒（revalidates == trusts+stales+captured；rebuilds == stales+captured）。
    if !d.audit() {
        return Err(E_BAL);
    }
    if d.stat.rebuilds != d.stat.stales + d.stat.captured {
        return Err(E_BAL);
    }
    if d.stat.revalidates != d.stat.trusts + d.stat.stales + d.stat.captured {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
