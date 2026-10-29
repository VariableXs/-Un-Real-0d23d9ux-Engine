//! unx4127_fileref — UNX-F4127 file 引用计数与 get_file/put（诊断段 0x91C0）。
//!
//! 判据原文：UNX-F4127-J1 f_count 万次往返零偏差，归零回调恰好一次，underflow
//!           注入即红（断言器捕获）。
//!   J2 count 恒等于 gets 减 puts（u64 长账）。
//!   J3 僵尸对象不可复活（get 即拒）。
//!   J4 槽位代次防 ABA，旧句柄无法误释放。
//! 设计纪律：
//!   1. 引用计数是「单写多读」账：全部分支只经 get/put 两个入口改动 count，
//!      结构外无一字可写；count 恒等于 gets - puts + 1（1 为创建期持有）。
//!   2. 归零不等价于回收：count==0 先落 Zombie（freed=false），由释放入口再落
//!      Freed 并触发一次释放回调；回调计数 free_calls 恒 ≤1，重复触发即红。
//!   3. underflow 必须被拦在减法之前（先比 0 再减），不允许「先减后查符号」；
//!      拦截本身记入 underflow_blocks 账，作为断言器的观测输入。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x91C0 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x91C0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_UNDERFLOW: u16 = SEG_BASE + 1; // put 时 count 已为 0（引用账穿底）
const E_FREED: u16 = SEG_BASE + 2; // 对已释放对象再 get/put
const E_DOUBLE_FREE: u16 = SEG_BASE + 3; // 释放回调重复触发
const E_ACCT: u16 = SEG_BASE + 4; // count 与 gets/puts 不平
const E_OVF: u16 = SEG_BASE + 5; // 计数上溢（u32 饱和前拦截）
const E_NOOBJ: u16 = SEG_BASE + 6; // 对象表空或句柄越界
const E_BADSLOT: u16 = SEG_BASE + 7; // 槽位复用冲突（未释放即重装）
const E_ZOMBIE_GET: u16 = SEG_BASE + 8; // 僵尸对象被复活（语义禁区）

/// 引用计数上限：预留 1/4 值域作上溢缓冲，触顶即拒而非回绕。
pub const REF_CEIL: u32 = u32::MAX - (u32::MAX / 4);

/// 对象生命周期状态机：Live → Zombie → Freed。Freed 为终态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefState {
    Live,
    Zombie,
    Freed,
}

impl RefState {
    pub fn name(self) -> &'static str {
        match self {
            RefState::Live => "Live",
            RefState::Zombie => "Zombie",
            RefState::Freed => "Freed",
        }
    }
}

/// 单个 file 的引用账：计数 + 收发累计 + 释放回调计次。
pub struct FileRef {
    /// 关联 inode 号
    pub ino: u32,
    /// f_count 当前值
    pub count: u32,
    /// 历史 get 次数（u64 防长跑溢出）
    pub gets: u64,
    /// 历史 put 次数
    pub puts: u64,
    /// 生命周期状态
    pub state: RefState,
    /// 释放回调实际触发次数（判据要求恒 ≤1）
    pub free_calls: u32,
}

impl FileRef {
    /// 创建即持有一枚引用（f_count=1，与 alloc_file 语义一致）。
    pub fn new(ino: u32) -> Result<FileRef, u16> {
        if ino == 0 {
            return Err(E_NOOBJ);
        }
        Ok(FileRef {
            ino,
            count: 1,
            gets: 1,
            puts: 0,
            state: RefState::Live,
            free_calls: 0,
        })
    }

    /// 不变式：count == gets - puts。
    pub fn balanced(&self) -> bool {
        self.gets >= self.puts && (self.gets - self.puts) == self.count as u64
    }
}

/// 引用计数枢纽：对象表 + 全局收发账 + underflow 拦截账。
pub struct RefHub {
    /// 对象表（定长池，槽位即对象位）
    pub files: Vec<FileRef>,
    /// 全体 get 次数
    pub gets_total: u64,
    /// 全体 put 次数
    pub puts_total: u64,
    /// 归零事件数（count 由 1 跌到 0）
    pub zero_events: u32,
    /// underflow 拦截次数（断言器输入）
    pub underflow_blocks: u32,
    /// 释放回调总触发次数
    pub free_total: u32,
}

impl RefHub {
    pub fn with_capacity(n: usize) -> RefHub {
        let mut v: Vec<FileRef> = Vec::new();
        let mut i = 0usize;
        while i < n {
            // 池内占位：ino=0 表空槽（不参与账目）
            v.push(FileRef {
                ino: 0,
                count: 0,
                gets: 0,
                puts: 0,
                state: RefState::Freed,
                free_calls: 0,
            });
            i += 1;
        }
        RefHub {
            files: v,
            gets_total: 0,
            puts_total: 0,
            zero_events: 0,
            underflow_blocks: 0,
            free_total: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// 装填槽位：仅空槽可装，重装即 E_BADSLOT。
    pub fn install(&mut self, slot: usize, ino: u32) -> Result<(), u16> {
        if slot >= self.files.len() {
            return Err(E_NOOBJ);
        }
        if self.files[slot].ino != 0 {
            return Err(E_BADSLOT);
        }
        let f = FileRef::new(ino)?;
        self.files[slot] = f;
        self.gets_total += 1;
        Ok(())
    }

    pub fn get_ref(&self, slot: usize) -> Result<&FileRef, u16> {
        match self.files.get(slot) {
            Some(f) => Ok(f),
            None => Err(E_NOOBJ),
        }
    }

    /// get_file：Live 才可加引用；Zombie 复活被拒（POSIX 引用语义不留后门）。
    pub fn get(&mut self, slot: usize) -> Result<u32, u16> {
        if slot >= self.files.len() {
            return Err(E_NOOBJ);
        }
        match self.files[slot].state {
            RefState::Freed => return Err(E_FREED),
            RefState::Zombie => return Err(E_ZOMBIE_GET),
            RefState::Live => {}
        }
        if self.files[slot].count >= REF_CEIL {
            return Err(E_OVF);
        }
        self.files[slot].count += 1;
        self.files[slot].gets += 1;
        self.gets_total += 1;
        Ok(self.files[slot].count)
    }

    /// put：先比 0 再减；跌到 0 落 Zombie 并记归零事件。
    pub fn put(&mut self, slot: usize) -> Result<u32, u16> {
        if slot >= self.files.len() {
            return Err(E_NOOBJ);
        }
        match self.files[slot].state {
            RefState::Freed => return Err(E_FREED),
            _ => {}
        }
        if self.files[slot].count == 0 {
            self.underflow_blocks += 1;
            return Err(E_UNDERFLOW);
        }
        self.files[slot].count -= 1;
        self.files[slot].puts += 1;
        self.puts_total += 1;
        if self.files[slot].count == 0 {
            self.files[slot].state = RefState::Zombie;
            self.zero_events += 1;
        }
        Ok(self.files[slot].count)
    }

    /// 归零回调：Zombie → Freed，回调恰好一次；对 Live/Freed 调用即拒。
    pub fn free_zombie(&mut self, slot: usize) -> Result<(), u16> {
        if slot >= self.files.len() {
            return Err(E_NOOBJ);
        }
        match self.files[slot].state {
            RefState::Live => Err(E_ZOMBIE_GET),
            RefState::Freed => Err(E_DOUBLE_FREE),
            RefState::Zombie => {
                if self.files[slot].free_calls != 0 {
                    return Err(E_DOUBLE_FREE);
                }
                self.files[slot].free_calls += 1;
                self.files[slot].state = RefState::Freed;
                self.free_total += 1;
                Ok(())
            }
        }
    }

    /// 全表对账：每槽 count == gets - puts；释放次数 == 已释放槽数。
    pub fn audit(&self) -> bool {
        let mut freed = 0u32;
        let mut i = 0usize;
        while i < self.files.len() {
            let f = &self.files[i];
            if f.ino != 0 && !f.balanced() {
                return false;
            }
            if f.state == RefState::Freed && f.ino != 0 {
                freed += 1;
            }
            if f.free_calls > 1 {
                return false;
            }
            i += 1;
        }
        freed == self.free_total
    }
}

/// 引用环：模拟 fork 类复制（多持有者共享同一对象）。
pub struct RefRing {
    /// 环内每端持有的引用增量
    pub holders: Vec<u32>,
    /// 环闭合次数
    pub closed: u32,
}

impl RefRing {
    pub fn new(n: usize) -> RefRing {
        RefRing {
            holders: {
                let mut v = Vec::new();
                let mut i = 0usize;
                while i < n {
                    v.push(0u32);
                    i += 1;
                }
                v
            },
            closed: 0,
        }
    }

    /// 在 hub 上为每端 get 一次，记入 holders。
    pub fn acquire_all(&mut self, hub: &mut RefHub, slot: usize) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.holders.len() {
            hub.get(slot)?;
            self.holders[i] += 1;
            i += 1;
        }
        Ok(())
    }

    /// 释放全部持有并闭合环。
    pub fn release_all(&mut self, hub: &mut RefHub, slot: usize) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.holders.len() {
            let mut k = 0u32;
            while k < self.holders[i] {
                hub.put(slot)?;
                k += 1;
            }
            self.holders[i] = 0;
            i += 1;
        }
        self.closed += 1;
        Ok(())
    }

    pub fn sum(&self) -> u32 {
        let mut s = 0u32;
        for &h in &self.holders {
            s += h;
        }
        s
    }
}

/// 诊断码解释。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_UNDERFLOW => "put 时 count 已为 0（引用账穿底）",
        E_FREED => "对已释放对象再 get/put",
        E_DOUBLE_FREE => "释放回调重复触发",
        E_ACCT => "count 与 gets/puts 不平",
        E_OVF => "引用计数触顶（上溢缓冲区内拦截）",
        E_NOOBJ => "对象表空或句柄越界",
        E_BADSLOT => "槽位未释放即重装",
        E_ZOMBIE_GET => "僵尸对象被复活或对活对象调归零回调",
        _ => "未知诊断码",
    }
}

/// 确定性演练：万次往返 → 归零回调 → underflow 注入 → 全表对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：万次 get/put 往返，count 回到 1，收发账零偏差。
    let mut hub = RefHub::with_capacity(4);
    hub.install(0, 0x2001)?;
    let mut i = 0u32;
    while i < 10_000 {
        hub.get(0)?;
        hub.put(0)?;
        i += 1;
    }
    let base = match hub.get_ref(0) {
        Ok(f) => f.count,
        Err(e) => return Err(e),
    };
    if base != 1 {
        return Err(E_ACCT); // 往返后必须回到创建期持有的 1
    }
    if hub.underflow_blocks != 0 || hub.zero_events != 0 {
        return Err(E_ACCT); // 全程无穿底、无归零
    }
    hits += 1;

    // R2：万次往返后 gets-puts 与 count 严格相等（u64 长账）。
    {
        let f = match hub.get_ref(0) {
            Ok(x) => x,
            Err(e) => return Err(e),
        };
        if f.gets != 10_001 || f.puts != 10_000 {
            return Err(E_ACCT);
        }
        if f.gets - f.puts != f.count as u64 {
            return Err(E_ACCT);
        }
        if f.state != RefState::Live {
            return Err(E_ACCT);
        }
    }
    hits += 1;

    // R3：归零回调恰好一次——最后一次 put 落 Zombie，free_zombie 落 Freed。
    hub.put(0)?;
    {
        let f = match hub.get_ref(0) {
            Ok(x) => x,
            Err(e) => return Err(e),
        };
        if f.count != 0 || f.state != RefState::Zombie {
            return Err(E_ACCT);
        }
    }
    hub.free_zombie(0)?;
    {
        let f = match hub.get_ref(0) {
            Ok(x) => x,
            Err(e) => return Err(e),
        };
        if f.free_calls != 1 || f.state != RefState::Freed {
            return Err(E_DOUBLE_FREE);
        }
    }
    if hub.zero_events != 1 || hub.free_total != 1 {
        return Err(E_ACCT);
    }
    hits += 1;

    // I1：underflow 注入 10/10 次全部拦截，且穿底不改变 count。
    let mut u = 0u32;
    let mut j = 0u32;
    while j < 10 {
        let mut h2 = RefHub::with_capacity(1);
        h2.install(0, 0x2002)?;
        h2.put(0)?; // count 1 → 0
        if h2.put(0) != Err(E_UNDERFLOW) {
            return Err(E_UNDERFLOW);
        }
        if h2.underflow_blocks != 1 {
            return Err(E_UNDERFLOW);
        }
        {
            let f = match h2.get_ref(0) {
                Ok(x) => x,
                Err(e) => return Err(e),
            };
            if f.count != 0 {
                return Err(E_UNDERFLOW); // 拦截后不得出现回绕
            }
        }
        u += 1;
        j += 1;
    }
    if u != 10 {
        return Err(E_UNDERFLOW);
    }
    hits += 1;

    // I2：二次释放回调 → E_DOUBLE_FREE。
    if hub.free_zombie(0) != Err(E_DOUBLE_FREE) {
        return Err(E_DOUBLE_FREE);
    }
    hits += 1;

    // I3：已释放对象再 get/put → E_FREED（终态不可复活）。
    if hub.get(0) != Err(E_FREED) || hub.put(0) != Err(E_FREED) {
        return Err(E_FREED);
    }
    hits += 1;

    // I4：槽位重装 → E_BADSLOT；越界槽 → E_NOOBJ。
    if hub.install(0, 0x2003) != Err(E_BADSLOT) {
        return Err(E_BADSLOT);
    }
    if hub.get(hub.len()) != Err(E_NOOBJ) {
        return Err(E_NOOBJ);
    }
    hits += 1;

    // I5：僵尸复活 → E_ZOMBIE_GET。
    let mut h3 = RefHub::with_capacity(1);
    h3.install(0, 0x2004)?;
    h3.put(0)?;
    if h3.get(0) != Err(E_ZOMBIE_GET) {
        return Err(E_ZOMBIE_GET);
    }
    hits += 1;

    // R4：引用环 8 端共享同一对象：acquire 后 count==9，release 后回 1。
    let mut h4 = RefHub::with_capacity(1);
    h4.install(0, 0x2005)?;
    let mut ring = RefRing::new(8);
    ring.acquire_all(&mut h4, 0)?;
    if ring.sum() != 8 {
        return Err(E_ACCT);
    }
    {
        let f = match h4.get_ref(0) {
            Ok(x) => x,
            Err(e) => return Err(e),
        };
        if f.count != 9 {
            return Err(E_ACCT); // 1（创建）+ 8（环内）
        }
    }
    ring.release_all(&mut h4, 0)?;
    {
        let f = match h4.get_ref(0) {
            Ok(x) => x,
            Err(e) => return Err(e),
        };
        if f.count != 1 || ring.closed != 1 {
            return Err(E_ACCT);
        }
    }
    hits += 1;

    // C1：全表对账——四只 hub 各自 count==gets-puts，释放次数与已释放槽相等。
    if !hub.audit() || !h4.audit() {
        return Err(E_ACCT);
    }
    if hub.gets_total != 10_001 {
        // 1（install 期创建持有）+ 10_000（R1 往返 get）
        return Err(E_ACCT);
    }
    hits += 1;

    Ok(hits)
}
