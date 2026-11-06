//! UNX-F9222 dup/dup2/dup3 坏 fd 矩阵 · 0x7A500段
//!
//! 判据原文（逐字摘录）：
//! ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9222-J1 oldfd/newfd 坏值注入矩阵判据过，
//! 全 EBADF 逐格检出，dup2 equal-fd 返回 newfd 正判据格过；
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 两层表：fd 表（32 槽，指向打开文件描述）+ OFD 表（41 槽，带引用计数），
//!    dup 家族的真实语义全在「引用数怎么增、怎么减、什么时候归零」上。
//! 2. 三兄弟三套判定序：dup 先老后容量；dup2 先老后同号再新号；
//!    dup3 先 flags、后同号、再老、最后新号——次序错了码就错。
//! 3. dup2 equal-fd 是恒等操作：oldfd 合法时 dup2(0,0) 返回 0；dup3 同号却是
//!    EINVAL，两兄弟在同一输入上给出相反结论，本卡必须逐格钉死。
//! 4. CLOEXEC 是逐 fd 标志：dup3 只污染 newfd，dup/dup2 产出的新号一律清零。
//! 5. 坏 old / 坏 new / 同号三类各注入 10 轮，检出 + 漏检守恒且漏检为 0。
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址（派工表给定，0x7A500 独占 0x40 格）。
pub const SEG_BASE: u32 = 0x7A500;
/// fd 表软限：newfd ≥ 此值即越界。
pub const FD_LIMIT: i32 = 32;
/// fd 槽容量。
pub const FD_CAP: usize = FD_LIMIT as usize;
/// 打开文件描述表容量，刻意大于 fd 表，保证先撞 fd 表。
pub const OFD_CAP: usize = 41;
/// 每类违约注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 注入类别数。
pub const INJECT_CLASSES: usize = 3;
/// 坏 fd 统一 errno。
pub const EBADF: i32 = 9;
/// 参数非法统一 errno（dup3 同号 / 坏 flags）。
pub const EINVAL: i32 = 22;
/// fd 表耗尽统一 errno。
pub const EMFILE: i32 = 24;

/// 诊断码闭集：dup 家族被写错时的十三种形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DupCode {
    BadOldAccepted,
    BadNewAccepted,
    EqualMisjudged,
    FlagMisjudged,
    CloexecLeak,
    AliasSplit,
    RefLeak,
    SlotOverwrite,
    OrderViolation,
    TableOverrun,
    CountMismatch,
    NotReproducible,
    SegmentViolation,
}

impl DupCode {
    /// 诊断码值：本段基址 + 变体序号。
    pub fn val(self) -> u32 { SEG_BASE + self as u32 }
    /// 中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            DupCode::BadOldAccepted => "坏 oldfd 被放行：先查 oldfd 是否已开，未开一律 EBADF",
            DupCode::BadNewAccepted => "坏 newfd 被放行：负数或 ≥ 软限的目标号必须回 EBADF",
            DupCode::EqualMisjudged => "同号判错：dup2 同号返回 newfd，dup3 同号回 EINVAL，别写反",
            DupCode::FlagMisjudged => "dup3 flags 未校验：除 O_CLOEXEC 外的位一律 EINVAL",
            DupCode::CloexecLeak => "CLOEXEC 串味：dup3 只置 newfd，dup/dup2 产出必须清零",
            DupCode::AliasSplit => "别名断裂：dup 后两号必须指向同一打开文件描述",
            DupCode::RefLeak => "引用泄漏：每次 dup 加一、每次 close 减一，归零才释放 OFD",
            DupCode::SlotOverwrite => "目标槽覆盖未结算：dup2 先释放旧目标再绑定新描述",
            DupCode::OrderViolation => "判定次序倒置：dup3 的 flags/同号检查必须早于 oldfd",
            DupCode::TableOverrun => "容量越限：fd 表满时 dup 回 EMFILE，不许挤占既有槽",
            DupCode::CountMismatch => "账实不符：引用数之和必须等于打开的 fd 数",
            DupCode::NotReproducible => "双跑不一致：同脚本两次指纹必须相等",
            DupCode::SegmentViolation => "段位越界：SEG_BASE 须 0x40 对齐且在 0x7A000..=0x7D1C0",
        }
    }
    /// 全码表。
    pub const ALL: [DupCode; 13] = [
        DupCode::BadOldAccepted, DupCode::BadNewAccepted, DupCode::EqualMisjudged,
        DupCode::FlagMisjudged, DupCode::CloexecLeak, DupCode::AliasSplit,
        DupCode::RefLeak, DupCode::SlotOverwrite, DupCode::OrderViolation,
        DupCode::TableOverrun, DupCode::CountMismatch, DupCode::NotReproducible,
        DupCode::SegmentViolation,
    ];
}

/// 打开文件描述：被多少个 fd 共同持有。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ofd {
    pub ino: u16,
    pub refs: u16,
}

impl Ofd {
    pub const fn empty() -> Ofd { Ofd { ino: 0, refs: 0 } }
    pub const fn live(self) -> bool { self.refs > 0 }
}

/// fd 绑定：指向哪个 OFD + 世代 + 逐 fd 的 CLOEXEC。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Binding {
    pub ofd: i16,
    pub gen: u16,
    pub cloexec: bool,
}

impl Binding {
    pub const fn free() -> Binding { Binding { ofd: -1, gen: 0, cloexec: false } }
    pub const fn is_open(self) -> bool { self.ofd >= 0 }
}

/// dup3 的 flags 取值域。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DupFlags {
    None,
    Cloexec,
    Unknown,
}

impl DupFlags {
    /// 只有 0 与 O_CLOEXEC 合法。
    pub const fn valid(self) -> bool { !matches!(self, DupFlags::Unknown) }
    pub const fn wants_cloexec(self) -> bool { matches!(self, DupFlags::Cloexec) }
}

/// 拒绝原因闭集：NotRejected 表示成功路径。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reject {
    NotRejected,
    BadFlags,
    SameFd,
    BadOld,
    BadNew,
    Emfile,
}

impl Reject {
    /// 映射回 errno：成功为 0。
    pub const fn errno(self) -> i32 {
        match self {
            Reject::NotRejected => 0,
            Reject::BadFlags => EINVAL,
            Reject::SameFd => EINVAL,
            Reject::BadOld => EBADF,
            Reject::BadNew => EBADF,
            Reject::Emfile => EMFILE,
        }
    }
    /// 原因名（写账用）。
    pub const fn label(self) -> &'static str {
        match self {
            Reject::NotRejected => "ok", Reject::BadFlags => "bad-flags",
            Reject::SameFd => "same-fd", Reject::BadOld => "bad-old",
            Reject::BadNew => "bad-new", Reject::Emfile => "emfile",
        }
    }
}

/// dup 家族的两层表。
#[derive(Clone, Copy, Debug)]
pub struct DupTable {
    fds: [Binding; FD_CAP],
    ofds: [Ofd; OFD_CAP],
    next_ino: u16,
    dups: u32,
    closes: u32,
    rejects: u32,
}

impl DupTable {
    pub const fn new() -> DupTable {
        DupTable {
            fds: [Binding::free(); FD_CAP],
            ofds: [Ofd::empty(); OFD_CAP],
            next_ino: 1,
            dups: 0,
            closes: 0,
            rejects: 0,
        }
    }
    /// 绑定视图。
    pub fn slot(&self, fd: i32) -> Option<Binding> {
        if fd < 0 { return None; }
        self.fds.get(fd as usize).copied()
    }
    /// 是否已开。
    pub fn is_open(&self, fd: i32) -> bool {
        matches!(self.slot(fd), Some(b) if b.is_open())
    }
    /// 世代。
    pub fn gen_of(&self, fd: i32) -> u16 {
        match self.slot(fd) { Some(b) => b.gen, None => 0 }
    }
    /// 逐 fd 的 CLOEXEC。
    pub fn cloexec(&self, fd: i32) -> bool {
        matches!(self.slot(fd), Some(b) if b.cloexec)
    }
    /// 引用数：坏 fd 为 0。
    pub fn refs(&self, fd: i32) -> u16 {
        match self.ofd_index(fd) {
            Some(idx) => match self.ofds.get(idx) { Some(o) => o.refs, None => 0 },
            None => 0,
        }
    }
    /// 两号是否指向同一打开文件描述。
    pub fn same_ofd(&self, a: i32, b: i32) -> bool {
        match (self.ofd_index(a), self.ofd_index(b)) {
            (Some(x), Some(y)) => x == y,
            _ => false,
        }
    }
    fn ofd_index(&self, fd: i32) -> Option<usize> {
        match self.slot(fd) {
            Some(b) if b.is_open() => Some(b.ofd as usize),
            _ => None,
        }
    }
    /// 打开计数。
    pub fn open_count(&self) -> u32 {
        (0..FD_CAP).filter_map(|i| self.fds.get(i)).filter(|b| b.is_open()).count() as u32
    }
    /// 活着的 OFD 数。
    pub fn live_ofds(&self) -> u32 {
        (0..OFD_CAP).filter_map(|i| self.ofds.get(i)).filter(|o| o.live()).count() as u32
    }
    /// 引用数总和。
    pub fn refs_total(&self) -> u32 {
        (0..OFD_CAP).filter_map(|i| self.ofds.get(i)).map(|o| o.refs as u32).sum()
    }
    /// 分配一个 OFD 槽并置引用 1。
    fn alloc_ofd(&mut self) -> Result<i16, Reject> {
        let mut i = 0usize;
        while i < OFD_CAP {
            let live = match self.ofds.get(i) { Some(o) => o.live(), None => true };
            if !live {
                let ino = self.next_ino.saturating_add(1);
                self.next_ino = ino;
                if let Some(o) = self.ofds.get_mut(i) { *o = Ofd { ino, refs: 1 }; }
                return Ok(i as i16);
            }
            i += 1;
        }
        Err(Reject::Emfile)
    }
    /// 打开新文件：取最低空 fd，先分配 OFD 再落绑定。
    pub fn open(&mut self) -> Result<i32, Reject> {
        let mut fd = 0i32;
        while fd < FD_LIMIT {
            if !self.is_open(fd) {
                let ofd = self.alloc_ofd()?;
                if let Some(b) = self.fds.get_mut(fd as usize) {
                    let gen = b.gen.saturating_add(1);
                    *b = Binding { ofd, gen, cloexec: false };
                }
                return Ok(fd);
            }
            fd += 1;
        }
        Err(Reject::Emfile)
    }
    /// 解绑一个 fd：引用减一，槽落空但保留世代。不记关闭账。
    fn unbind(&mut self, fd: i32) -> bool {
        let ofd = match self.ofd_index(fd) { Some(v) => v, None => return false };
        if let Some(o) = self.ofds.get_mut(ofd) { o.refs = o.refs.saturating_sub(1); }
        if let Some(b) = self.fds.get_mut(fd as usize) {
            let gen = b.gen.saturating_add(1);
            *b = Binding { ofd: -1, gen, cloexec: false };
        }
        true
    }
    /// 关闭：坏 fd 回 EBADF，成功记一笔关闭账。
    pub fn close(&mut self, fd: i32) -> Result<i32, Reject> {
        if !self.unbind(fd) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(Reject::BadOld);
        }
        self.closes = self.closes.saturating_add(1);
        Ok(0)
    }
    /// dup：仅复制到最低空槽，新号 CLOEXEC 清零。
    pub fn dup(&mut self, oldfd: i32) -> Result<i32, Reject> {
        let ofd = match self.ofd_index(oldfd) { Some(v) => v, None => return Err(Reject::BadOld) };
        let mut fd = 0i32;
        while fd < FD_LIMIT {
            if !self.is_open(fd) {
                if let Some(o) = self.ofds.get_mut(ofd) { o.refs = o.refs.saturating_add(1); }
                if let Some(b) = self.fds.get_mut(fd as usize) {
                    let gen = b.gen.saturating_add(1);
                    *b = Binding { ofd: ofd as i16, gen, cloexec: false };
                }
                self.dups = self.dups.saturating_add(1);
                return Ok(fd);
            }
            fd += 1;
        }
        Err(Reject::Emfile)
    }
    /// dup2：老号 → 指定新号；同号恒等返回 newfd。
    pub fn dup2(&mut self, oldfd: i32, newfd: i32) -> Result<i32, Reject> {
        if !self.is_open(oldfd) { return Err(Reject::BadOld); }
        if oldfd == newfd { return Ok(newfd); }
        if newfd < 0 || newfd >= FD_LIMIT { return Err(Reject::BadNew); }
        let ofd = match self.ofd_index(oldfd) { Some(v) => v, None => return Err(Reject::BadOld) };
        if self.is_open(newfd) { let _ = self.unbind(newfd); }
        if let Some(o) = self.ofds.get_mut(ofd) { o.refs = o.refs.saturating_add(1); }
        if let Some(b) = self.fds.get_mut(newfd as usize) {
            let gen = b.gen.saturating_add(1);
            *b = Binding { ofd: ofd as i16, gen, cloexec: false };
        }
        self.dups = self.dups.saturating_add(1);
        Ok(newfd)
    }
    /// dup3：flags → 同号 → 老号 → 新号，四关次序固定。
    pub fn dup3(&mut self, oldfd: i32, newfd: i32, flags: DupFlags) -> Result<i32, Reject> {
        if !flags.valid() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(Reject::BadFlags);
        }
        if oldfd == newfd {
            self.rejects = self.rejects.saturating_add(1);
            return Err(Reject::SameFd);
        }
        if !self.is_open(oldfd) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(Reject::BadOld);
        }
        if newfd < 0 || newfd >= FD_LIMIT {
            self.rejects = self.rejects.saturating_add(1);
            return Err(Reject::BadNew);
        }
        let ofd = match self.ofd_index(oldfd) { Some(v) => v, None => return Err(Reject::BadOld) };
        if self.is_open(newfd) { let _ = self.unbind(newfd); }
        if let Some(o) = self.ofds.get_mut(ofd) { o.refs = o.refs.saturating_add(1); }
        if let Some(b) = self.fds.get_mut(newfd as usize) {
            let gen = b.gen.saturating_add(1);
            *b = Binding { ofd: ofd as i16, gen, cloexec: flags.wants_cloexec() };
        }
        self.dups = self.dups.saturating_add(1);
        Ok(newfd)
    }
    /// 双账计数。
    pub fn totals(&self) -> (u32, u32, u32) { (self.dups, self.closes, self.rejects) }
    /// 两层表指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < FD_CAP {
            if let Some(b) = self.fds.get(i) {
                bytes.push((b.ofd + 1) as u8);
                bytes.push((b.gen & 0xFF) as u8);
                bytes.push(if b.cloexec { 1 } else { 0 });
            }
            i += 1;
        }
        i = 0usize;
        while i < OFD_CAP {
            if let Some(o) = self.ofds.get(i) {
                bytes.push((o.refs & 0xFF) as u8);
                bytes.push((o.ino & 0xFF) as u8);
            }
            i += 1;
        }
        bytes.push((self.dups & 0xFF) as u8);
        bytes.push((self.closes & 0xFF) as u8);
        fnv1a(&bytes)
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        let (dups, closes, rejects) = self.totals();
        format!("dup 表：开 {} / dup {} / 关 {} / 拒 {} / 活 OFD {}",
                self.open_count(), dups, closes, rejects, self.live_ofds())
    }
}

/// 注入类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DupInject {
    BadOld,
    BadNew,
    SameFd,
}

impl DupInject {
    /// 该类别应落到的拒绝原因。
    pub const fn expected(self) -> Reject {
        match self {
            DupInject::BadOld => Reject::BadOld,
            DupInject::BadNew => Reject::BadNew,
            DupInject::SameFd => Reject::SameFd,
        }
    }
    /// 三类全表。
    pub const ALL: [DupInject; INJECT_CLASSES] = [
        DupInject::BadOld, DupInject::BadNew, DupInject::SameFd,
    ];
}

/// 注入账簿。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DupLedger {
    pub bad_old: u32,
    pub bad_new: u32,
    pub same_fd: u32,
    pub detects: u32,
    pub misses: u32,
}

impl DupLedger {
    pub const fn empty() -> DupLedger {
        DupLedger { bad_old: 0, bad_new: 0, same_fd: 0, detects: 0, misses: 0 }
    }
    /// 记一笔。
    pub fn account(&mut self, expected: Reject, actual: Reject, hit: bool) {
        if hit && actual == expected {
            match actual {
                Reject::BadOld => self.bad_old = self.bad_old.saturating_add(1),
                Reject::BadNew => self.bad_new = self.bad_new.saturating_add(1),
                Reject::SameFd => self.same_fd = self.same_fd.saturating_add(1),
                _ => {}
            }
            self.detects = self.detects.saturating_add(1);
        } else {
            self.misses = self.misses.saturating_add(1);
        }
    }
    /// 守恒：检出 + 漏检 = 注入总数。
    pub fn conserved(&self) -> bool {
        self.detects.saturating_add(self.misses) == INJECT_ROUNDS * INJECT_CLASSES as u32
            && self.bad_old.saturating_add(self.bad_new).saturating_add(self.same_fd) == self.detects
    }
    /// 分类计数。
    pub fn per(&self, reject: Reject) -> u32 {
        match reject {
            Reject::BadOld => self.bad_old,
            Reject::BadNew => self.bad_new,
            Reject::SameFd => self.same_fd,
            _ => 0,
        }
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!("坏 old {} / 坏 new {} / 同号 {} / 检出 {} / 漏检 {}",
                self.bad_old, self.bad_new, self.same_fd, self.detects, self.misses)
    }
}

/// 单轮注入：三类各自把三条调用打满。
pub fn inject_once(kind: DupInject, round: u32) -> (Reject, bool) {
    let mut table = DupTable::new();
    let _ = table.open();
    let bad = -1 - (round as i32 % 7);
    let high = FD_LIMIT + 1 + (round as i32 % 3);
    let got = match kind {
        DupInject::BadOld => {
            let a = table.dup(bad);
            let b = table.dup2(bad, 5);
            let c = table.dup3(bad, 6, DupFlags::None);
            if a == Err(Reject::BadOld) && b == Err(Reject::BadOld) && c == Err(Reject::BadOld) {
                Reject::BadOld
            } else {
                Reject::NotRejected
            }
        }
        DupInject::BadNew => {
            let a = table.dup2(0, bad);
            let b = table.dup3(0, high, DupFlags::Cloexec);
            if a == Err(Reject::BadNew) && b == Err(Reject::BadNew) {
                Reject::BadNew
            } else {
                Reject::NotRejected
            }
        }
        DupInject::SameFd => {
            let a = table.dup3(0, 0, DupFlags::None);
            let b = table.dup2(0, 0);
            if a == Err(Reject::SameFd) && b == Ok(0) {
                Reject::SameFd
            } else {
                Reject::NotRejected
            }
        }
    };
    (got, got == kind.expected())
}

/// 三类 × 10 轮矩阵。
pub fn run_dup_matrix() -> (DupLedger, [u32; INJECT_CLASSES]) {
    let mut ledger = DupLedger::empty();
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut i = 0usize;
        while i < INJECT_CLASSES {
            let kind = match DupInject::ALL.get(i) { Some(k) => *k, None => DupInject::BadOld };
            let (actual, hit) = inject_once(kind, round);
            ledger.account(kind.expected(), actual, hit);
            i += 1;
        }
        round += 1;
    }
    let per = [ledger.bad_old, ledger.bad_new, ledger.same_fd];
    (ledger, per)
}

/// 判定次序探针：dup3 的 flags 与同号都排在 oldfd 之前；dup2 则先查 oldfd。
pub fn judge_order_probe() -> (Reject, Reject, Reject) {
    let mut table = DupTable::new();
    let _ = table.open();
    let first = match table.dup3(-1, -1, DupFlags::Unknown) {
        Err(e) => e,
        Ok(_) => Reject::NotRejected,
    };
    let second = match table.dup3(20, 20, DupFlags::None) {
        Err(e) => e,
        Ok(_) => Reject::NotRejected,
    };
    let third = match table.dup2(20, 20) {
        Err(e) => e,
        Ok(_) => Reject::NotRejected,
    };
    (first, second, third)
}

/// 对账行：调用式 / 期望 errno / 符号 / Windows 码。
#[derive(Clone, Copy, Debug)]
pub struct ReconRow {
    pub call: &'static str,
    pub errno: i32,
    pub name: &'static str,
    pub win: u16,
}

/// 对账表八行：覆盖坏老号、坏新号、同号两兄弟、容量与成功路径。
pub const RECON_TABLE: [ReconRow; 8] = [
    ReconRow { call: "dup(-1)", errno: EBADF, name: "EBADF", win: 6 },
    ReconRow { call: "dup2(-1,5)", errno: EBADF, name: "EBADF", win: 6 },
    ReconRow { call: "dup2(0,0)", errno: 0, name: "OK", win: 0 },
    ReconRow { call: "dup3(0,0,CLOEXEC)", errno: EINVAL, name: "EINVAL", win: 87 },
    ReconRow { call: "dup3(0,1,UNKNOWN)", errno: EINVAL, name: "EINVAL", win: 87 },
    ReconRow { call: "dup2(0,32)", errno: EBADF, name: "EBADF", win: 6 },
    ReconRow { call: "dup3(0,32,CLOEXEC)", errno: EBADF, name: "EBADF", win: 6 },
    ReconRow { call: "dup3(0,1,CLOEXEC)", errno: 0, name: "OK", win: 0 },
];

/// 成功为 0，失败取拒绝原因的 errno。
pub fn errno_of(result: Result<i32, Reject>) -> i32 {
    match result {
        Ok(_) => 0,
        Err(reject) => reject.errno(),
    }
}

/// 按行重放对账场景。
pub fn replay(case: usize) -> i32 {
    let mut table = DupTable::new();
    let _ = table.open();
    let _ = table.open();
    match case {
        0 => errno_of(table.dup(-1)),
        1 => errno_of(table.dup2(-1, 5)),
        2 => errno_of(table.dup2(0, 0)),
        3 => errno_of(table.dup3(0, 0, DupFlags::Cloexec)),
        4 => errno_of(table.dup3(0, 1, DupFlags::Unknown)),
        5 => errno_of(table.dup2(0, FD_LIMIT)),
        6 => errno_of(table.dup3(0, FD_LIMIT, DupFlags::Cloexec)),
        7 => errno_of(table.dup3(0, 1, DupFlags::Cloexec)),
        _ => -1,
    }
}

/// 逐行重算：返回（一致行数, 总行数）。
pub fn reconcile() -> (u32, u32) {
    let mut ok = 0u32;
    let mut i = 0usize;
    while i < RECON_TABLE.len() {
        if let Some(row) = RECON_TABLE.get(i) {
            if replay(i) == row.errno { ok = ok.saturating_add(1); }
        }
        i += 1;
    }
    (ok, RECON_TABLE.len() as u32)
}

/// 失配明细。
pub fn recon_mismatch_text() -> String {
    let mut text = String::new();
    let mut i = 0usize;
    while i < RECON_TABLE.len() {
        if let Some(row) = RECON_TABLE.get(i) {
            let got = replay(i);
            if got != row.errno {
                text.push_str(&format!("第{}行 {} 表={} 实={}; ", i + 1, row.call, row.name, got));
            }
        }
        i += 1;
    }
    if text.is_empty() { text.push_str("8 行逐项一致"); }
    text
}

/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(byte) = bytes.get(i) {
            hash ^= *byte as u32;
            hash = hash.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    hash
}

/// 巡逻脚本：固定 dup/dup2/dup3/close 序列。
pub fn patrol_fingerprint() -> u32 {
    let mut table = DupTable::new();
    let _ = table.open();
    let _ = table.open();
    let _ = table.open();
    let _ = table.dup2(0, 5);
    let _ = table.dup3(0, 6, DupFlags::Cloexec);
    let _ = table.close(5);
    let _ = table.dup(0);
    table.fingerprint()
}

/// 写回摘要。
pub fn dup_summary() -> String {
    let (ok, total) = reconcile();
    let (ledger, _) = run_dup_matrix();
    format!("F9222 dup 矩阵：对账 {}/{} / 注入 检{} 漏{} / fd 限 {} / OFD 槽 {}",
            ok, total, ledger.detects, ledger.misses, FD_LIMIT, OFD_CAP)
}

/// 本卡判据入口。
pub fn run_unx9222_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9222");

    s.add("段位：0x7A500 0x40 对齐且落在 0x7A000..=0x7D1C0 内",
          SEG_BASE == 0x7A500 && SEG_BASE % 0x40 == 0
              && SEG_BASE >= 0x7A000 && SEG_BASE <= 0x7D1C0,
          "段位独占，越界即整册返工");

    let mut basic = DupTable::new();
    let first = basic.open();
    let second = basic.open();
    let dup_low = basic.dup(0);
    s.add("正常流：dup 复制到最低空槽且两号共享同一 OFD",
          first == Ok(0) && second == Ok(1) && dup_low == Ok(2)
              && basic.same_ofd(0, 2) && basic.refs(0) == 2 && basic.open_count() == 3,
          "dup 必须取最低空槽并加引用，别名关系不能断");

    let mut to_new = DupTable::new();
    let _ = to_new.open();
    let bound = to_new.dup2(0, 9);
    s.add("正常流：dup2 绑到指定新号并返回 newfd",
          bound == Ok(9) && to_new.same_ofd(0, 9) && to_new.refs(9) == 2
              && !to_new.cloexec(9),
          "dup2 目标号由调用方指定，返回的就是 newfd");

    let mut eq = DupTable::new();
    let _ = eq.open();
    let before = eq.refs(0);
    s.add("dup2 equal-fd 正判据：dup2(0,0) == Ok(0) 且引用不增",
          eq.dup2(0, 0) == Ok(0) && eq.refs(0) == before && eq.open_count() == 1,
          "dup2 同号是恒等操作，必须返回 newfd 而不是 EINVAL");
    s.add("dup3 equal-fd 反向判据：同号回 EINVAL",
          eq.dup3(0, 0, DupFlags::None) == Err(Reject::SameFd),
          "dup3 的同号是参数错误，且要排在 oldfd 判定之前");

    let mut flags = DupTable::new();
    let _ = flags.open();
    let _ = flags.open();
    s.add("dup3 flags：CLOEXEC 只落 newfd，旧号不受污染",
          flags.dup3(0, 5, DupFlags::Cloexec) == Ok(5)
              && flags.cloexec(5) && !flags.cloexec(0)
              && flags.dup3(0, 6, DupFlags::Unknown) == Err(Reject::BadFlags),
          "cloexec 是逐 fd 标志，dup3 只在新号上置位，未知位一律 EINVAL");

    let mut bounds = DupTable::new();
    let _ = bounds.open();
    s.add("边界：oldfd −1/软限 → EBADF；newfd −1/软限 → EBADF；限内最高槽可绑",
          bounds.dup(-1) == Err(Reject::BadOld)
              && bounds.dup(FD_LIMIT) == Err(Reject::BadOld)
              && bounds.dup2(0, -1) == Err(Reject::BadNew)
              && bounds.dup2(0, FD_LIMIT) == Err(Reject::BadNew)
              && bounds.dup3(0, -1, DupFlags::None) == Err(Reject::BadNew)
              && bounds.dup2(0, FD_LIMIT - 1) == Ok(FD_LIMIT - 1),
          "负数与软限及以上都是坏号；软限-1 必须可绑");

    let mut badnew = DupTable::new();
    let _ = badnew.open();
    s.add("边界：dup3 老号合法但新号越界 → BadNew（不误判老号）",
          badnew.dup3(0, FD_LIMIT + 1, DupFlags::None) == Err(Reject::BadNew)
              && badnew.dup3(0, FD_LIMIT, DupFlags::None) == Err(Reject::BadNew),
          "老号过关后新号才轮到判，越界一律 EBADF");

    let mut alias = DupTable::new();
    let _ = alias.open();
    let _ = alias.open();
    let clone = match alias.dup(0) { Ok(v) => v, Err(_) => -1 };
    let refs_two = alias.refs(0);
    let _ = alias.close(0);
    let refs_one = alias.refs(clone);
    let _ = alias.close(clone);
    s.add("引用账：dup 后 refs=2，关一个减一，全关归零",
          clone == 2 && refs_two == 2 && refs_one == 1
              && alias.refs_total() == 1 && alias.live_ofds() == 1,
          "引用数只随 dup 增、随 close 减，归零才是真正释放");

    let mut clobber = DupTable::new();
    let _ = clobber.open();
    let _ = clobber.open();
    let live_before = clobber.live_ofds();
    let rebind = clobber.dup2(0, 1);
    s.add("状态机：dup2 目标已开 → 先释放旧 OFD 再绑定",
          rebind == Ok(1) && clobber.same_ofd(0, 1)
              && clobber.live_ofds() == live_before.saturating_sub(1),
          "目标槽原有文件必须被静默释放，引用不能悬空");

    let mut full = DupTable::new();
    let mut n = 0i32;
    while n < FD_LIMIT {
        let _ = full.open();
        n += 1;
    }
    s.add("容量：fd 表 32 槽满 → open/dup 回 EMFILE，dup2 重绑仍成功",
          full.open() == Err(Reject::Emfile)
              && full.dup(0) == Err(Reject::Emfile)
              && full.dup2(0, FD_LIMIT - 1) == Ok(FD_LIMIT - 1),
          "dup 要新槽故回 EMFILE；dup2 不扩表，满表也能绑到已开目标");
    s.add("守恒：refs 之和 == 打开 fd 数",
          full.refs_total() == full.open_count() && full.open_count() == FD_LIMIT as u32,
          "每个 fd 恰好持有一份引用，总和必须与打开数相等");

    let (ledger, per) = run_dup_matrix();
    s.add("注入必抓：坏 old / 坏 new / 同号三类各 10/10",
          matches!(per.get(0), Some(v) if *v == INJECT_ROUNDS)
              && matches!(per.get(1), Some(v) if *v == INJECT_ROUNDS)
              && matches!(per.get(2), Some(v) if *v == INJECT_ROUNDS),
          "三类违约各 10 轮全检出，漏一轮即红");
    s.add("守恒：注入 30 次 = 检出 + 漏检，漏检为 0",
          ledger.detects == INJECT_ROUNDS * INJECT_CLASSES as u32
              && ledger.misses == 0 && ledger.conserved(),
          "账簿自洽：检出与漏检之和恒等于注入轮数");

    let (r1, r2, r3) = judge_order_probe();
    s.add("判定次序：dup3 flags > 同号 > oldfd；dup2 先 oldfd",
          r1 == Reject::BadFlags && r2 == Reject::SameFd && r3 == Reject::BadOld,
          "flags 与同号同时违约时 dup3 先回 EINVAL；dup2 同号先看老号是否已开");

    let mut order2 = DupTable::new();
    let _ = order2.open();
    s.add("判定次序：dup3 老号坏 + 新号越界 → BadOld（先老后新）",
          order2.dup3(20, FD_LIMIT + 2, DupFlags::None) == Err(Reject::BadOld),
          "老号检测必须早于新号范围检测，否则回码不一致");

    let f1 = patrol_fingerprint();
    let f2 = patrol_fingerprint();
    s.add("双跑一致：同脚本两次指纹相等且非零", f1 == f2 && f1 != 0,
          "无时钟无随机，同输入必须逐字段同指纹");

    let (rok, rtotal) = reconcile();
    s.add("对账：8 行 dup/dup2/dup3 回码与对账表一致",
          rok == rtotal && rtotal == 8,
          "EBADF/EINVAL/EMFILE 与 Windows 6/87/4 对表，逐行可复算");

    let mut text_closed = true;
    let mut d = 0usize;
    while let Some(code) = DupCode::ALL.get(d) {
        if code.detail().is_empty() { text_closed = false; }
        if code.val() < SEG_BASE || code.val() > SEG_BASE + 0x3F { text_closed = false; }
        d += 1;
    }
    s.add("文案闭集：13 个诊断码均有非空中文建议且落本段", text_closed,
          "只报码不给路等于没报，诊断码必须自带处置建议");

    let summary = dup_summary();
    s.add("收口：摘要非空且含对账/注入/限值三组数",
          !summary.is_empty() && summary.contains("对账")
              && summary.contains("注入") && summary.contains("fd 限"),
          "写回摘要必须一眼看出对账与注入结果");

    let mut dyn_table = DupTable::new();
    let _ = dyn_table.open();
    let bad_probe = dyn_table.dup(-1);
    let bad_label = match bad_probe { Err(e) => e.label(), Ok(_) => "ok" };
    s.add_dyn("坏老号明细", bad_probe == Err(Reject::BadOld),
              &format!("dup(-1) → {}（errno {}）", bad_label, errno_of(bad_probe)));
    let eq_probe = {
        let _ = dyn_table.open();
        (dyn_table.dup2(0, 0), dyn_table.refs(0))
    };
    s.add_dyn("dup2 同号明细", eq_probe.0 == Ok(0),
              &format!("dup2(0,0) → {:?} / refs {}", eq_probe.0, eq_probe.1));
    let recon_text = recon_mismatch_text();
    s.add_dyn("对账失配明细", recon_text.starts_with("8 行"), &recon_text);
    s.add_dyn("注入账簿", ledger.conserved() && ledger.misses == 0, &ledger.summary());

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dup_lowest_free_and_alias() {
        let mut table = DupTable::new();
        assert_eq!(table.open(), Ok(0));
        assert_eq!(table.open(), Ok(1));
        assert_eq!(table.dup(0), Ok(2));
        assert!(table.same_ofd(0, 2));
        assert_eq!(table.refs(0), 2);
        assert_eq!(table.refs_total(), 3);
        assert_eq!(table.open_count(), 3);
    }

    #[test]
    fn dup2_equal_returns_newfd() {
        let mut table = DupTable::new();
        assert_eq!(table.open(), Ok(0));
        assert_eq!(table.dup2(0, 0), Ok(0));
        assert_eq!(table.refs(0), 1);
        assert_eq!(table.open_count(), 1);
        assert_eq!(table.dup2(-1, -1), Err(Reject::BadOld));
    }

    #[test]
    fn dup3_order_and_flags() {
        let mut table = DupTable::new();
        assert_eq!(table.open(), Ok(0));
        assert_eq!(table.open(), Ok(1));
        assert_eq!(table.dup3(-1, -1, DupFlags::Unknown), Err(Reject::BadFlags));
        assert_eq!(table.dup3(20, 20, DupFlags::None), Err(Reject::SameFd));
        assert_eq!(table.dup3(20, 30, DupFlags::None), Err(Reject::BadOld));
        assert_eq!(table.dup3(0, 5, DupFlags::Cloexec), Ok(5));
        assert!(table.cloexec(5));
        assert!(!table.cloexec(0));
        assert_eq!(table.dup3(0, 6, DupFlags::Unknown), Err(Reject::BadFlags));
    }

    #[test]
    fn bounds_matrix() {
        let mut table = DupTable::new();
        assert_eq!(table.open(), Ok(0));
        assert_eq!(table.dup(-1), Err(Reject::BadOld));
        assert_eq!(table.dup(FD_LIMIT), Err(Reject::BadOld));
        assert_eq!(table.dup2(0, -1), Err(Reject::BadNew));
        assert_eq!(table.dup2(0, FD_LIMIT), Err(Reject::BadNew));
        assert_eq!(table.dup3(0, FD_LIMIT, DupFlags::None), Err(Reject::BadNew));
        assert_eq!(table.dup2(0, FD_LIMIT - 1), Ok(FD_LIMIT - 1));
        assert!(table.gen_of(FD_LIMIT - 1) > 0);
    }

    #[test]
    fn refcount_lifecycle() {
        let mut table = DupTable::new();
        assert_eq!(table.open(), Ok(0));
        assert_eq!(table.open(), Ok(1));
        assert_eq!(table.dup(0), Ok(2));
        assert_eq!(table.refs(0), 2);
        assert_eq!(table.close(0), Ok(0));
        assert_eq!(table.refs(2), 1);
        assert_eq!(table.live_ofds(), 2);
        assert_eq!(table.close(1), Ok(0));
        assert_eq!(table.close(2), Ok(0));
        assert_eq!(table.live_ofds(), 0);
        assert_eq!(table.close(2), Err(Reject::BadOld));
    }

    #[test]
    fn dup2_clobbers_target() {
        let mut table = DupTable::new();
        assert_eq!(table.open(), Ok(0));
        assert_eq!(table.open(), Ok(1));
        assert_eq!(table.live_ofds(), 2);
        assert_eq!(table.dup2(0, 1), Ok(1));
        assert!(table.same_ofd(0, 1));
        assert_eq!(table.live_ofds(), 1);
        assert_eq!(table.refs(1), 2);
    }

    #[test]
    fn fd_table_full() {
        let mut table = DupTable::new();
        let mut n = 0i32;
        while n < FD_LIMIT {
            assert!(table.open().is_ok());
            n += 1;
        }
        assert_eq!(table.open_count(), FD_LIMIT as u32);
        assert_eq!(table.open(), Err(Reject::Emfile));
        assert_eq!(table.dup(0), Err(Reject::Emfile));
        assert_eq!(table.dup2(0, FD_LIMIT - 1), Ok(FD_LIMIT - 1));
        assert_eq!(table.refs_total(), table.open_count());
    }

    #[test]
    fn injection_and_conservation() {
        let (ledger, per) = run_dup_matrix();
        assert_eq!(ledger.detects, INJECT_ROUNDS * INJECT_CLASSES as u32);
        assert_eq!(ledger.misses, 0);
        assert!(ledger.conserved());
        assert_eq!(per, [INJECT_ROUNDS, INJECT_ROUNDS, INJECT_ROUNDS]);
        assert_eq!(ledger.per(Reject::BadOld), INJECT_ROUNDS);
        assert_eq!(ledger.per(Reject::BadNew), INJECT_ROUNDS);
        assert_eq!(ledger.per(Reject::SameFd), INJECT_ROUNDS);
        assert_eq!(
            judge_order_probe(),
            (Reject::BadFlags, Reject::SameFd, Reject::BadOld)
        );
    }

    #[test]
    fn determinism_recon_text() {
        assert_eq!(patrol_fingerprint(), patrol_fingerprint());
        assert_ne!(patrol_fingerprint(), 0);
        let (ok, total) = reconcile();
        assert_eq!(ok, total);
        assert_eq!(total, 8);
        assert!(recon_mismatch_text().starts_with("8 行"));
        assert!(dup_summary().contains("对账"));
        assert_eq!(DupCode::ALL.len(), 13);
        let mut i = 0usize;
        while let Some(code) = DupCode::ALL.get(i) {
            assert!(!code.detail().is_empty());
            assert!(code.val() >= SEG_BASE && code.val() <= SEG_BASE + 0x3F);
            i += 1;
        }
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x7A000..=0x7D1C0).contains(&SEG_BASE));
    }
}
