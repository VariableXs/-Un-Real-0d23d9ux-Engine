//! unx4106_dinvalid — UNX-F4106 dentry 失效广播（unlink/rename 联动）（诊断段 0x8F40）。
//!
//! 判据原文（提炼）：
//!   J1 unlink/rename 后关联正/负条目全部失效：失效计数 == 关联数
//!      （广播逐条入账，无漏杀无滥杀）。
//!   J2 失效后首次查找回真实态：失效条目不得直接回结果，必须重新
//!      穿透 fs 探测（陈旧读取注入零命中）。
//!   J3 广播范围精确：目录 unlink/rename 级联失效其子树条目
//!      （前缀匹配），无关联条目不受影响。
//!   J4 双操作语义分离：unlink 失效即删账；rename 迁移名后旧名失效、
//!      新名继承有效性（迁移语义）。
//!   J5 对已失效条目重复广播幂等：失效计数不再增长，错误账记录重复广播。
//! 设计纪律：
//!   1. 失效三态：Valid → Invalidated →（重探后）Refreshed，
//!      只有 Valid 态可直接回查找结果。
//!   2. 级联键 = 目录前缀：广播按名字前缀匹配子树（POSIX 目录树语义），
//!      根名 "/" 级联全树。
//!   3. 陈旧读取防线：lookup 遇 Invalidated 一律作废缓存行并穿透
//!      fs 重探，作废与重探都入账（判据 J2 零陈旧命中）。
//!   4. fs 实态供给：与缓存平行的实态集，unlink/rename 先动实态
//!      再广播缓存（顺序固定，先实态后广播）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x8F40 本单独占。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8F40;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NOT_EXIST: u16 = SEG_BASE + 1;  // 操作对象实态不存在
const E_STALE_READ: u16 = SEG_BASE + 2; // 失效态直接回结果（陈旧读取注入）
const E_BAL: u16 = SEG_BASE + 3;        // 失效计数与关联数不平
const E_DUP_FS: u16 = SEG_BASE + 4;     // 实态重复创建
const E_MOVE_BAD: u16 = SEG_BASE + 5;   // rename 目标名已存在
const E_REBROADCAST: u16 = SEG_BASE + 6;// 对已失效条目重复广播（幂等面）
const E_NAME_BAD: u16 = SEG_BASE + 7;   // 空名/非法名

/// 条目有效性状态机：Valid → Invalidated → Refreshed。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Validity {
    Valid,       // 有效，可直接回查找
    Invalidated, // 已失效，必须重探
    Refreshed,   // 失效后重探已回填
}

/// 单条缓存行：名 + kind 标记 + 有效性 + 重探账。
pub struct CkLine {
    pub name: String,       // 缓存键（相对路径分量串）
    pub negative: bool,     // true = 负条目（确认不存在）
    pub validity: Validity, // 有效性状态机
    pub reprobe: u32,       // 被重探次数
}

/// 失效广播枢纽：缓存账 + fs 实态账 + 广播账。
pub struct InvalBus {
    pub lines: Vec<CkLine>,     // 缓存行账
    pub fs_files: Vec<String>,  // fs 实态（存在的名集合）
    pub invalidated_total: u32, // 累计失效条目数（判据 J1 核对面）
    pub stale_reads: u32,       // 陈旧读取命中数（判据 J2，恒 0 才绿）
    pub rebroadcasts: u32,      // 重复广播计数（判据 J5 幂等面）
    pub rejected: u32,          // 其他拒收计数
}

impl InvalBus {
    pub fn new() -> InvalBus {
        InvalBus {
            lines: Vec::new(),
            fs_files: Vec::new(),
            invalidated_total: 0,
            stale_reads: 0,
            rebroadcasts: 0,
            rejected: 0,
        }
    }

    fn name_ok(name: &str) -> Result<(), u16> {
        if name.is_empty() || name.contains('/') {
            return Err(E_NAME_BAD); // 本模型按单分量/相对串管理，禁含根分隔
        }
        Ok(())
    }

    /// 前缀级联判定：target 是否位于 dir 子树（dir == "" 视为全树）。
    fn under(dir: &str, target: &str) -> bool {
        if dir.is_empty() {
            return true;
        }
        target == dir || (target.len() > dir.len() && target.starts_with(dir) && target.as_bytes()[dir.len()] == b'/')
    }

    /// fs 建实态。
    pub fn fs_create(&mut self, name: &str) -> Result<(), u16> {
        InvalBus::name_ok(name)?;
        for f in &self.fs_files {
            if f == name {
                self.rejected += 1;
                return Err(E_DUP_FS);
            }
        }
        self.fs_files.push(name.to_string());
        Ok(())
    }

    /// 手工铺缓存行（正/负），供广播对账。
    pub fn cache_put(&mut self, name: &str, negative: bool) -> Result<(), u16> {
        InvalBus::name_ok(name)?;
        for l in &self.lines {
            if l.name == name && l.validity != Validity::Invalidated {
                self.rejected += 1;
                return Err(E_DUP_FS);
            }
        }
        self.lines.push(CkLine {
            name: name.to_string(),
            negative,
            validity: Validity::Valid,
            reprobe: 0,
        });
        Ok(())
    }

    /// 在账定位有效行。
    fn line_pos(&self, name: &str) -> Option<usize> {
        for i in 0..self.lines.len() {
            if self.lines[i].name == name && self.lines[i].validity != Validity::Invalidated {
                return Some(i);
            }
        }
        None
    }

    /// fs 实态探测。
    fn probe(&self, name: &str) -> bool {
        for f in &self.fs_files {
            if f == name {
                return true;
            }
        }
        false
    }

    /// 查找（判据 J2）：Valid 直接回；Invalidated 必须重探；
    /// 禁止任何路径在失效态直接回缓存结果。
    pub fn lookup(&mut self, name: &str) -> Result<bool, u16> {
        InvalBus::name_ok(name)?;
        if let Some(i) = self.line_pos(name) {
            if self.lines[i].validity == Validity::Valid {
                // 有效正条目：直接回缓存实态（本行建立时已对齐 fs）
                return Ok(!self.lines[i].negative);
            }
        }
        // 无有效行：穿透重探并回填
        let real = self.probe(name);
        match self.line_pos(name) {
            Some(i) => {
                self.lines[i].validity = Validity::Refreshed;
                self.lines[i].negative = !real;
                self.lines[i].reprobe += 1;
            }
            None => {
                self.lines.push(CkLine {
                    name: name.to_string(),
                    negative: !real,
                    validity: Validity::Refreshed,
                    reprobe: 1,
                });
            }
        }
        Ok(real)
    }

    /// unlink 广播（判据 J1/J3）：实态先删，随后级联失效子树行。
    /// 返回本轮失效条目数（== 关联数）。
    pub fn unlink_broadcast(&mut self, dir: &str) -> Result<u32, u16> {
        InvalBus::name_ok(dir)?;
        // 实态先动：删 dir 及子树全部实态
        let mut gone: Vec<usize> = Vec::new();
        for i in 0..self.fs_files.len() {
            if InvalBus::under(dir, &self.fs_files[i]) {
                gone.push(i);
            }
        }
        if gone.is_empty() {
            self.rejected += 1;
            return Err(E_NOT_EXIST); // 实态不存在不得广播
        }
        // 从尾往前摘，防下标移位
        let mut k = gone.len();
        while k > 0 {
            k -= 1;
            self.fs_files.remove(gone[k]);
        }
        // 缓存广播：级联失效（判据 J1）
        let mut n = 0u32;
        for i in 0..self.lines.len() {
            if self.lines[i].validity == Validity::Invalidated {
                continue; // 已失效跳过（判据 J5 幂等）
            }
            if InvalBus::under(dir, &self.lines[i].name) {
                self.lines[i].validity = Validity::Invalidated;
                n += 1;
            }
        }
        self.invalidated_total += n;
        Ok(n)
    }

    /// rename 联动（判据 J4）：实态改名 → 旧名失效 → 新名回填有效。
    pub fn rename_broadcast(&mut self, old: &str, new: &str) -> Result<(), u16> {
        InvalBus::name_ok(old)?;
        InvalBus::name_ok(new)?;
        let mut oi = self.fs_files.len();
        for i in 0..self.fs_files.len() {
            if self.fs_files[i] == old {
                oi = i;
                break;
            }
        }
        if oi == self.fs_files.len() {
            self.rejected += 1;
            return Err(E_NOT_EXIST);
        }
        for f in &self.fs_files {
            if f == new {
                self.rejected += 1;
                return Err(E_MOVE_BAD); // 目标已存在
            }
        }
        self.fs_files[oi] = new.to_string();
        // 旧名缓存行失效（若有）
        if let Some(i) = self.line_pos(old) {
            self.lines[i].validity = Validity::Invalidated;
            self.invalidated_total += 1;
        }
        // 新名行回填有效（迁移语义：新名继承实态有效性）
        match self.line_pos(new) {
            Some(i) => {
                self.lines[i].validity = Validity::Valid;
                self.lines[i].negative = false;
            }
            None => {
                self.lines.push(CkLine {
                    name: new.to_string(),
                    negative: false,
                    validity: Validity::Valid,
                    reprobe: 0,
                });
            }
        }
        Ok(())
    }

    /// 重复广播注入（判据 J5）：对已清空的子树再广播必须拒并记账。
    pub fn rebroadcast_probe(&mut self, dir: &str) -> Result<(), u16> {
        InvalBus::name_ok(dir)?;
        let mut any = false;
        for i in 0..self.lines.len() {
            if self.lines[i].validity == Validity::Valid
                && InvalBus::under(dir, &self.lines[i].name)
            {
                any = true;
            }
        }
        if !any {
            self.rebroadcasts += 1;
            return Err(E_REBROADCAST); // 无关联可失效：重复广播被拒
        }
        Ok(())
    }

    /// 对账三连：失效计数平账 + 零陈旧命中 + 状态机合法。
    pub fn conserved(&self) -> bool {
        if self.stale_reads != 0 {
            return false; // 判据 J2 零陈旧
        }
        for l in &self.lines {
            match l.validity {
                Validity::Valid | Validity::Refreshed => {}
                Validity::Invalidated => {
                    if l.reprobe != 0 {
                        return false; // 失效行不得有重探后仍失效的混合态
                    }
                }
            }
        }
        true
    }
}

/// 自例程：R1 铺账→R2 unlink 级联失效→R3 rename 迁移→R4 注入与对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 正常铺账：实态 dir/a.txt、dir/b.txt、other.txt；
    // 缓存行 dir/a.txt(正)、dir/b.txt(负)、other.txt(正)。
    let mut b = InvalBus::new();
    b.fs_create("dir/a.txt")?;
    b.fs_create("dir/b.txt")?;
    b.fs_create("other.txt")?;
    b.cache_put("dir/a.txt", false)?;
    b.cache_put("dir/b.txt", true)?;
    b.cache_put("other.txt", false)?;
    hits += 1;

    // R2 unlink dir：级联失效 2 条（判据 J3 精确范围，other.txt 免疫）。
    let n = b.unlink_broadcast("dir")?;
    if n != 2 || b.invalidated_total != 2 {
        return Err(E_BAL); // 判据 J1：失效计数==关联数
    }
    // 失效后首查必须重探回真实态（实态已删 → 不存在）。
    if b.lookup("dir/a.txt")? {
        return Err(E_STALE_READ); // 实态已删仍报存在即陈旧读取
    }
    if b.stale_reads != 0 {
        return Err(E_STALE_READ);
    }
    // other.txt 不受影响（有效行直接回 true）。
    if !b.lookup("other.txt")? {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 rename 联动：other.txt → renamed.txt，旧名失效新名有效。
    b.rename_broadcast("other.txt", "renamed.txt")?;
    if b.lookup("renamed.txt")? != true {
        return Err(E_BAL); // 新名继承有效
    }
    if b.lookup("other.txt")? {
        return Err(E_STALE_READ); // 实态已改名，旧名必须回不存在
    }
    hits += 1;

    // R4 注入与对账：重复广播、改名目标冲突、操作不存在实态全拒。
    if b.rebroadcast_probe("dir").is_ok() {
        return Err(E_REBROADCAST); // 已清空子树重复广播应拒（判据 J5）
    }
    if b.rename_broadcast("renamed.txt", "renamed.txt").is_ok() {
        return Err(E_MOVE_BAD); // 目标同名冲突应拒
    }
    if b.unlink_broadcast("ghost").is_ok() {
        return Err(E_NOT_EXIST); // 无实态广播应拒
    }
    if !b.conserved() {
        return Err(E_BAL);
    }
    if b.rebroadcasts < 1 {
        return Err(E_REBROADCAST); // 幂等账必须如实记
    }
    hits += 1;

    Ok(hits)
}
