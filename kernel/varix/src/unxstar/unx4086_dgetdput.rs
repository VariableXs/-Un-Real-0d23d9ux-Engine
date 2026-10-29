//! unx4086_dgetdput — UNX-F4086 dget/dput 引用纪律（诊断段 0x8CE0）。
//!
//! 判据原文：UNX-F4086-J1 d_count 与持有者账逐笔对应万次零偏差，对负 dentry 的
//!           dget 行为与正 dentry 一致（统一纪律断言通过）。
//! 可验性质（本模块自证面）：
//!   J1 万次 dget/dput 配对后，每条 dentry 的 d_count 恒等于其持有者表长度，
//!      且 Σd_count == Σ各持有者表长度（逐笔对应，零偏差）；
//!   J2 负 dentry（dcache negative）的 dget/dput 与正 dentry 完全同规：同样的
//!      计数、同样的持有者校验、同样的归零路径，不得因 negative 而走旁路；
//!   J3 引用计数归零即可回收：count 归 0 时态转 Unreferenced，再 dget 回到
//!      Referenced（shrink 后复活），计数上限 max_count 有账；
//!   J4 错误路径全拒且不入账：对未在世（Freed）dentry dget → E_FREED，
//!      count 为 0 时 dput → E_UNDERFLOW，持有者不符 → E_HOLDER（不扣他人账）；
//!   J5 单持有者账："谁 dget 谁 dput"：释放时持有者 id 必须在表内，
//!      否则拒；同一持有者可多次持有（重入），表内以重复项表达。
//! 设计纪律：
//!   1. d_count 是唯一权威计数，持有者表只作对账证据：先改 count 再同步表，
//!      两步同进同退；两处不等即视为账坏（E_BAL）。
//!   2. 正负 dentry 共用同一结构体与同一套 API（negative 只是标记位），
//!      禁止为负项另开一条代码路径——旁路就是纪律松动的入口。
//!   3. 归零不释放内存：态置 Unreferenced，等上层（dcache shrinker）决定回收；
//!      本模块只管计数纪律，不管回收时机（与 F4095 分工）。
//!   4. 持有者 id 用 u32（模块号/线程号编码），0 保留为"匿名持有者"，
//!      匿名持有者不允许 dput（查不到归属），只能由 dput_anon 显式归还。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!
//! 诊断段 0x8CE0 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8CE0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NO_DENTRY: u16 = SEG_BASE + 1; // dentry 号不在表内
const E_FREED: u16 = SEG_BASE + 2; // 对已释放 dentry dget
const E_UNDERFLOW: u16 = SEG_BASE + 3; // d_count 为 0 仍 dput
const E_HOLDER: u16 = SEG_BASE + 4; // 持有者不符（该 id 未持有）
const E_ANON: u16 = SEG_BASE + 5; // 匿名持有者不可具名 dput
const E_DUP_ID: u16 = SEG_BASE + 6; // dentry 号重复建项
const E_BAL: u16 = SEG_BASE + 7; // d_count 与持有者账不平
const E_CAP: u16 = SEG_BASE + 8; // 计数触顶（u32 上限前的硬上限）

/// 单 dentry 引用计数硬上限：正常 VFS 到不了，触顶即视为泄漏征兆。
pub const COUNT_CAP: u32 = 0x00FF_FFFF;

/// dentry 生命周期态（与计数联动）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DState {
    New = 0,          // 刚建，count=0，未引用
    Referenced = 1,   // count>0，被持有
    Unreferenced = 2, // count 归 0，待回收（可被 dget 复活）
    Freed = 3,        // 已释放，不可再引用
}

/// 单 dentry 的引用账。
pub struct DRef {
    pub id: u64,          // dentry 标识
    pub count: u32,       // d_count（权威计数）
    pub negative: bool,   // 是否负 dentry
    pub holders: Vec<u32>, // 持有者表（对账证据，允许重复项=重入）
    pub state: DState,    // 生命周期态
    pub peak: u32,        // 本 dentry 计数峰值
}

impl DRef {
    /// 持有者计数（含重入的重复项）。
    pub fn holder_len(&self) -> u32 {
        self.holders.len() as u32
    }

    /// 账自洽：count == holders.len()。
    pub fn consistent(&self) -> bool {
        self.count == self.holder_len()
    }

    /// 某持有者当前持有几笔。
    pub fn held_by(&self, h: u32) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.holders.len() {
            if self.holders[i] == h {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// 引用总账：dget/dput 笔数 + 错误账。
pub struct RefStat {
    pub dgets: u32,      // 成功 dget 笔数
    pub dputs: u32,      // 成功 dput 笔数
    pub underflows: u32, // 下溢拒收
    pub freed_hits: u32, // 对已释放项 dget 拒收
    pub holder_miss: u32, // 持有者不符拒收
    pub anon_puts: u32,  // 匿名归还笔数
    pub max_count: u32,  // 全表计数峰值
}

impl RefStat {
    fn new() -> RefStat {
        RefStat {
            dgets: 0,
            dputs: 0,
            underflows: 0,
            freed_hits: 0,
            holder_miss: 0,
            anon_puts: 0,
            max_count: 0,
        }
    }
}

/// 引用表：dentry 表 + 总账。
pub struct RefTable {
    pub ents: Vec<DRef>, // dentry 表
    pub stat: RefStat,   // 总账
}

impl RefTable {
    pub fn new() -> RefTable {
        RefTable {
            ents: Vec::new(),
            stat: RefStat::new(),
        }
    }

    /// 建项：号唯一、0 号拒（0 视为空 dentry 指针）。
    pub fn add(&mut self, id: u64, negative: bool) -> Result<(), u16> {
        if id == 0 {
            return Err(E_NO_DENTRY);
        }
        if self.find(id).is_some() {
            return Err(E_DUP_ID);
        }
        self.ents.push(DRef {
            id,
            count: 0,
            negative,
            holders: Vec::new(),
            state: DState::New,
            peak: 0,
        });
        Ok(())
    }

    /// 定位下标（查前比 len）。
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

    /// dget：负项与正项同一套判定，无旁路（J2）。
    pub fn dget(&mut self, id: u64, holder: u32) -> Result<u32, u16> {
        let idx = match self.find(id) {
            Some(i) => i,
            None => return Err(E_NO_DENTRY),
        };
        if self.ents[idx].state == DState::Freed {
            self.stat.freed_hits += 1;
            return Err(E_FREED);
        }
        if self.ents[idx].count >= COUNT_CAP {
            return Err(E_CAP);
        }
        if holder == 0 {
            return Err(E_ANON); // 匿名者必须走 dget_anon，不许混账
        }
        self.ents[idx].count += 1;
        self.ents[idx].holders.push(holder);
        self.ents[idx].state = DState::Referenced;
        if self.ents[idx].count > self.ents[idx].peak {
            self.ents[idx].peak = self.ents[idx].count;
        }
        if self.ents[idx].count > self.stat.max_count {
            self.stat.max_count = self.ents[idx].count;
        }
        self.stat.dgets += 1;
        if !self.ents[idx].consistent() {
            return Err(E_BAL);
        }
        Ok(self.ents[idx].count)
    }

    /// 匿名 dget（holder 记 0，只允许 dput_anon 归还）。
    pub fn dget_anon(&mut self, id: u64) -> Result<u32, u16> {
        let idx = match self.find(id) {
            Some(i) => i,
            None => return Err(E_NO_DENTRY),
        };
        if self.ents[idx].state == DState::Freed {
            self.stat.freed_hits += 1;
            return Err(E_FREED);
        }
        if self.ents[idx].count >= COUNT_CAP {
            return Err(E_CAP);
        }
        self.ents[idx].count += 1;
        self.ents[idx].holders.push(0);
        self.ents[idx].state = DState::Referenced;
        if self.ents[idx].count > self.ents[idx].peak {
            self.ents[idx].peak = self.ents[idx].count;
        }
        if self.ents[idx].count > self.stat.max_count {
            self.stat.max_count = self.ents[idx].count;
        }
        self.stat.dgets += 1;
        Ok(self.ents[idx].count)
    }

    /// dput：具名归还，持有者必须在表内（J5）。
    pub fn dput(&mut self, id: u64, holder: u32) -> Result<u32, u16> {
        let idx = match self.find(id) {
            Some(i) => i,
            None => return Err(E_NO_DENTRY),
        };
        if holder == 0 {
            return Err(E_ANON); // 匿名项请走 dput_anon
        }
        if self.ents[idx].count == 0 {
            self.stat.underflows += 1;
            return Err(E_UNDERFLOW);
        }
        let pos = self.find_holder(idx, holder);
        let pos = match pos {
            Some(p) => p,
            None => {
                self.stat.holder_miss += 1;
                return Err(E_HOLDER);
            }
        };
        self.ents[idx].holders.remove(pos);
        self.ents[idx].count -= 1;
        if self.ents[idx].count == 0 {
            self.ents[idx].state = DState::Unreferenced; // 归零待回收（J3）
        }
        self.stat.dputs += 1;
        if !self.ents[idx].consistent() {
            return Err(E_BAL);
        }
        Ok(self.ents[idx].count)
    }

    /// 匿名归还：只吃掉一笔 holder==0 的记录。
    pub fn dput_anon(&mut self, id: u64) -> Result<u32, u16> {
        let idx = match self.find(id) {
            Some(i) => i,
            None => return Err(E_NO_DENTRY),
        };
        if self.ents[idx].count == 0 {
            self.stat.underflows += 1;
            return Err(E_UNDERFLOW);
        }
        let pos = self.find_holder(idx, 0);
        let pos = match pos {
            Some(p) => p,
            None => {
                self.stat.holder_miss += 1;
                return Err(E_HOLDER);
            }
        };
        self.ents[idx].holders.remove(pos);
        self.ents[idx].count -= 1;
        if self.ents[idx].count == 0 {
            self.ents[idx].state = DState::Unreferenced;
        }
        self.stat.dputs += 1;
        self.stat.anon_puts += 1;
        Ok(self.ents[idx].count)
    }

    /// 持有者定位（首个匹配项，支持重入的重复项）。
    fn find_holder(&self, idx: usize, holder: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ents[idx].holders.len() {
            if self.ents[idx].holders[i] == holder {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 回收：仅 Unreferenced/New 可 Freed（count 必须为 0）。
    pub fn reclaim(&mut self, id: u64) -> Result<(), u16> {
        let idx = match self.find(id) {
            Some(i) => i,
            None => return Err(E_NO_DENTRY),
        };
        if self.ents[idx].count != 0 {
            return Err(E_BAL); // 还有人持有，不许回收
        }
        match self.ents[idx].state {
            DState::Unreferenced | DState::New => {
                self.ents[idx].state = DState::Freed;
                Ok(())
            }
            DState::Freed => Err(E_FREED),
            DState::Referenced => Err(E_BAL),
        }
    }

    /// 全表对账：逐条 count == holder_len（J1 的逐笔面）。
    pub fn audit(&self) -> bool {
        let mut i = 0usize;
        while i < self.ents.len() {
            if !self.ents[i].consistent() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 总账对账：Σcount == Σholder_len，且 dgets + max 与 dputs + Σcount 相容。
    pub fn totals_match(&self) -> bool {
        let mut sc = 0u32;
        let mut sh = 0u32;
        let mut i = 0usize;
        while i < self.ents.len() {
            sc += self.ents[i].count;
            sh += self.ents[i].holder_len();
            i += 1;
        }
        if sc != sh {
            return false;
        }
        // 成功 dget 与成功 dput 之差必须等于当前在持总数（供 dget_anon 一并计）
        self.stat.dgets >= self.stat.dputs
            && self.stat.dgets - self.stat.dputs == sc
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NO_DENTRY => "dentry 号不在表内（含 0 号）",
        E_FREED => "对已释放 dentry 再 dget",
        E_UNDERFLOW => "d_count 为 0 仍 dput",
        E_HOLDER => "持有者不符（该 id 未持有）",
        E_ANON => "匿名持有者不可混用具名 API",
        E_DUP_ID => "dentry 号重复建项",
        E_BAL => "d_count 与持有者账不平",
        E_CAP => "引用计数触顶（泄漏征兆）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：万次配对 → 负项同规 → 归零复活 → 注入拦截 → 总账对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：建正负各一条，万次"dget 后 dput"配对，每条结束后 count 归零（J1）。
    let mut t = RefTable::new();
    t.add(0x10, false)?;
    t.add(0x11, true)?; // 负 dentry
    let mut i = 0u32;
    while i < 10_000 {
        let holder = 1 + (i % 16);
        t.dget(0x10, holder)?;
        if t.find(0x10).map(|x| t.ents[x].count) != Some(1) {
            return Err(E_BAL);
        }
        t.dput(0x10, holder)?;
        if t.find(0x10).map(|x| t.ents[x].count) != Some(0) {
            return Err(E_BAL);
        }
        i += 1;
    }
    if t.stat.dgets != 10_000 || t.stat.dputs != 10_000 {
        return Err(E_BAL);
    }
    if !t.audit() || !t.totals_match() {
        return Err(E_BAL);
    }
    if t.find(0x10).map(|x| t.ents[x].state) != Some(DState::Unreferenced) {
        return Err(E_BAL); // 归零即 Unreferenced
    }
    if t.find(0x10).map(|x| t.ents[x].peak) != Some(1) {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：负 dentry 同规——万次配对跑在 0x11 上，行为与正项逐笔相同（J2）。
    i = 0;
    while i < 10_000 {
        let holder = 1 + (i % 8);
        t.dget(0x11, holder)?;
        t.dput(0x11, holder)?;
        i += 1;
    }
    if t.find(0x11).map(|x| t.ents[x].negative) != Some(true) {
        return Err(E_BAL);
    }
    if t.find(0x11).map(|x| t.ents[x].count) != Some(0) {
        return Err(E_BAL);
    }
    if t.find(0x11).map(|x| t.ents[x].peak) != Some(1) {
        return Err(E_BAL); // 与正项同一峰值：纪律统一
    }
    if !t.audit() {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：多层持有——同一 dentry 被 4 个持有者同时持有，count == 4，逐笔可归。
    let holders = [11u32, 12, 13, 14];
    let mut h = 0usize;
    while h < 4 {
        t.dget(0x10, holders[h])?;
        h += 1;
    }
    if t.find(0x10).map(|x| t.ents[x].count) != Some(4) {
        return Err(E_BAL);
    }
    if t.find(0x10).map(|x| t.ents[x].peak) != Some(4) {
        return Err(E_BAL);
    }
    if t.stat.max_count != 4 {
        return Err(E_BAL);
    }
    // 逆序归：4→0，态回 Unreferenced
    h = 4;
    while h > 0 {
        h -= 1;
        t.dput(0x10, holders[h])?;
    }
    if t.find(0x10).map(|x| t.ents[x].count) != Some(0) {
        return Err(E_BAL);
    }
    if !t.totals_match() {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：重入持有——同持有者拿 3 笔，归 3 笔，账仍平（J5）。
    t.dget(0x10, 7)?;
    t.dget(0x10, 7)?;
    t.dget(0x10, 7)?;
    if t.find(0x10).map(|x| t.ents[x].held_by(7)) != Some(3) {
        return Err(E_BAL);
    }
    t.dput(0x10, 7)?;
    t.dput(0x10, 7)?;
    t.dput(0x10, 7)?;
    if t.find(0x10).map(|x| t.ents[x].held_by(7)) != Some(0) {
        return Err(E_BAL);
    }
    if !t.audit() || !t.totals_match() {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：匿名路——dget_anon + dput_anon 配对，笔数进 anon_puts，不污染具名账。
    t.dget_anon(0x11)?;
    if t.find(0x11).map(|x| t.ents[x].held_by(0)) != Some(1) {
        return Err(E_BAL);
    }
    t.dput_anon(0x11)?;
    if t.stat.anon_puts != 1 {
        return Err(E_BAL);
    }
    if t.find(0x11).map(|x| t.ents[x].count) != Some(0) {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：count 为 0 仍 dput → E_UNDERFLOW，不得出现 u32 回绕（J4）。
    if t.dput(0x10, 1) != Err(E_UNDERFLOW) {
        return Err(E_UNDERFLOW);
    }
    if t.stat.underflows != 1 {
        return Err(E_BAL);
    }
    if t.find(0x10).map(|x| t.ents[x].count) != Some(0) {
        return Err(E_BAL);
    }
    hits += 1;

    // I2：持有者不符 → E_HOLDER；别人持有时冒充归还必须被拒。
    t.dget(0x10, 21)?;
    if t.dput(0x10, 22) != Err(E_HOLDER) {
        return Err(E_HOLDER);
    }
    if t.stat.holder_miss != 1 {
        return Err(E_BAL);
    }
    if t.find(0x10).map(|x| t.ents[x].count) != Some(1) {
        return Err(E_BAL);
    }
    t.dput(0x10, 21)?;
    hits += 1;

    // I3：回收纪律——count 非 0 不许回收；回收后 dget 必拒（E_FREED）。
    t.dget(0x11, 5)?;
    if t.reclaim(0x11) != Err(E_BAL) {
        return Err(E_BAL);
    }
    t.dput(0x11, 5)?;
    t.reclaim(0x11)?;
    if t.dget(0x11, 5) != Err(E_FREED) {
        return Err(E_FREED);
    }
    if t.stat.freed_hits != 1 {
        return Err(E_BAL);
    }
    if t.reclaim(0x11) != Err(E_FREED) {
        return Err(E_FREED); // 重复回收也拒
    }
    hits += 1;

    // I4：0 号与重复建项、匿名混用三路拒收。
    if t.add(0, false) != Err(E_NO_DENTRY) {
        return Err(E_NO_DENTRY);
    }
    if t.add(0x10, false) != Err(E_DUP_ID) {
        return Err(E_DUP_ID);
    }
    if t.dget(0x10, 0) != Err(E_ANON) {
        return Err(E_ANON);
    }
    if t.dput(0x10, 0) != Err(E_ANON) {
        return Err(E_ANON);
    }
    hits += 1;

    // I5：未知 dentry 全 API 拒（dget/dput/reclaim 三路）。
    if t.dget(0xDEAD, 1) != Err(E_NO_DENTRY) {
        return Err(E_NO_DENTRY);
    }
    if t.dput(0xDEAD, 1) != Err(E_NO_DENTRY) {
        return Err(E_NO_DENTRY);
    }
    if t.reclaim(0xDEAD) != Err(E_NO_DENTRY) {
        return Err(E_NO_DENTRY);
    }
    hits += 1;

    // 终审：全表逐条账平 + 总账笔数平（dget - dput == 在持总数）。
    if !t.audit() || !t.totals_match() {
        return Err(E_BAL);
    }
    if t.find(0x10).map(|x| t.ents[x].count) != Some(0) {
        return Err(E_BAL);
    }
    if t.find(0x11).map(|x| t.ents[x].state) != Some(DState::Freed) {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
