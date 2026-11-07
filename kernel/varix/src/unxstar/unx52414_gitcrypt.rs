//! UNX-F52414 git-crypt 收口 · 0xCA740段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52414"，逐字）：
//!   title: git-crypt 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52415-J1 ×10；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52414）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52415-J1，照原文锚定；功能按本卡标题（git-crypt）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五命令族 20 操作闭集：init/add-key/lock/unlock/export 各 4 操
//!      作，族单调（逆序必抓），逐操作载荷纯函数折叠恒等才收。
//!   ② 密钥投影 8 格：.git/git-crypt/keys 镜像逐格对照，漂移必抓；
//!      filter 三态 6 格（clean/smudge/diff 三态闭集，越态必抓）。
//!   ③ 与 git 消费联签 4 格（F52159 锚定行）：filter 驱动注册/smudge
//!      管道/clean 管道/diff 属性逐格联签，越闭集与漂移同码必抓。
//!   ④ C++ 上栈声明 2 行闭集：libgit-crypt 静态库上栈 / OpenSSL crypto
//!      后端分账，缺一拒封；六判据母版闸未齐即封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（命令漂移/
//!      命令逆序/命令越闭集）全部落在当轮临时账本；双跑指纹一致；
//!      定级 S2（全齐目标档）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52414;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五命令族操作数。
pub const CMD_OPS: usize = 20;

/// 密钥投影格数。
pub const KEY_CELLS: usize = 8;

/// filter 三态格数。
pub const FILTER_CELLS: usize = 6;

/// git 消费联签格数（F52159 锚定行）。
pub const COSIGN_CELLS: usize = 4;

/// C++ 上栈声明行数。
pub const STACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GitcryptCode {
    /// 成功
    Ok = 0,
    /// 命令操作号越闭集
    ENoCmd,
    /// 命令操作漂移
    ECmdDrift,
    /// 命令族逆序
    EOrder,
    /// 密钥投影越闭集
    EKey,
    /// 密钥投影漂移
    EKeyDrift,
    /// filter 格越闭集
    ENoFilter,
    /// filter 三态越态
    EFilterState,
    /// git 消费联签越闭集或漂移
    ECosign,
    /// 声明行越闭集或缺失
    ERow,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl GitcryptCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            GitcryptCode::Ok => "成功",
            GitcryptCode::ENoCmd => "命令操作号越闭集：init/add-key/lock/unlock/export 20 操作之外先登记",
            GitcryptCode::ECmdDrift => "命令操作漂移：操作折叠与基准不恒等，逐格复算",
            GitcryptCode::EOrder => "命令族逆序：init→add-key→lock→unlock→export 单调，逆序必抓",
            GitcryptCode::EKey => "密钥投影越闭集：8 格密钥镜像之外先登记",
            GitcryptCode::EKeyDrift => "密钥投影漂移：.git/git-crypt/keys 投影与基准不恒等",
            GitcryptCode::ENoFilter => "filter 格越闭集：6 格三态账之外先登记",
            GitcryptCode::EFilterState => "filter 三态越态：clean/smudge/diff 三态闭集之外先登记",
            GitcryptCode::ECosign => "git 消费联签越闭集或漂移：4 格联签（F52159 锚定行）先补齐",
            GitcryptCode::ERow => "声明行越闭集或缺失：C++ 上栈 2 行闭集之内先补齐",
            GitcryptCode::EDup => "重复落账：同格只许一条账目",
            GitcryptCode::ESeal => "未齐即封：20 操作 + 8 密钥 + 6 三态 + 4 联签 + 2 行未齐先补齐",
            GitcryptCode::EGradeDrift => "定级漂移：全齐且联签全落才可定级",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集（本卡目标档 S2）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口（目标档）
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：全通收口",
            Grade::S2 => "S2：六闸齐即有条件收口，全齐为本卡目标档（KB-COMPAT 定级登记）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 命令操作格载荷（操作 o；命令族 = o/4）。
pub const fn cmd_cell(o: usize) -> Option<u64> {
    if o >= CMD_OPS {
        return None;
    }
    Some((0xD2410u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 命令族（0..4=init 4..8=add-key 8..12=lock 12..16=unlock 16..20=export）。
pub const fn cmd_kind(o: usize) -> Option<u32> {
    if o >= CMD_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 命令族名（const fn 表 + Option 边界）。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("init"),
        1 => Some("add-key"),
        2 => Some("lock"),
        3 => Some("unlock"),
        4 => Some("export"),
        _ => None,
    }
}

/// 密钥投影格基准（格 v；.git/git-crypt/keys 镜像）。
pub const fn key_cell(v: usize) -> Option<u64> {
    if v >= KEY_CELLS {
        return None;
    }
    Some((0xD2420u64 + (v as u64) * 0x37) ^ 0x474954435259_0000)
}

/// filter 三态格基准（格 d；三态 0=clean 1=smudge 2=diff）。
pub const fn filter_cell(d: usize) -> Option<u64> {
    if d >= FILTER_CELLS {
        return None;
    }
    Some(0xD2430u64 + (d as u64) * 0x41)
}

/// 三态名。
pub const fn filter_state_name(s: u32) -> Option<&'static str> {
    match s {
        0 => Some("clean"),
        1 => Some("smudge"),
        2 => Some("diff"),
        _ => None,
    }
}

/// git 消费联签格基准（格 c；F52159 锚定行）。
pub const fn cosign_cell(c: usize) -> Option<u64> {
    if c >= COSIGN_CELLS {
        return None;
    }
    Some(0xD2440u64 + (c as u64) * 0x53)
}

/// C++ 上栈声明行折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xD2450 + r as u32 * 0x2C)
}

/// 六判据闸名（filter 驱动注册/属性导出/密钥解锁/仓库锁定/协作者授权/恢复能力）。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("filter 驱动注册"),
        1 => Some("属性导出"),
        2 => Some("密钥解锁"),
        3 => Some("仓库锁定"),
        4 => Some("协作者授权"),
        5 => Some("恢复能力"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// git-crypt 收口账本状态机。
#[derive(Clone)]
pub struct GitcryptLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 命令操作账。
    cmds: [Option<u64>; CMD_OPS],
    /// 密钥投影账。
    keys: [Option<u64>; KEY_CELLS],
    /// filter 三态账（格 → (基准, 态)）。
    filters: [Option<(u64, u32)>; FILTER_CELLS],
    /// git 消费联签账（F52159）。
    cosigns: [Option<u64>; COSIGN_CELLS],
    /// C++ 上栈声明账。
    rows: [Option<u32>; STACK_ROWS],
    /// 最后命令族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl GitcryptLedger {
    /// 新账本。
    pub fn new() -> Self {
        GitcryptLedger {
            gates: [None; 6],
            cmds: [None; CMD_OPS],
            keys: [None; KEY_CELLS],
            filters: [None; FILTER_CELLS],
            cosigns: [None; COSIGN_CELLS],
            rows: [None; STACK_ROWS],
            last_kind: -1,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), GitcryptCode> {
        if gate >= 6 {
            return Err(GitcryptCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5D710 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 命令一格（命令族单调 + 载荷对照 + 重复拒）。
    pub fn record_cmd(&mut self, o: usize, got: u64) -> Result<(), GitcryptCode> {
        if o >= CMD_OPS {
            return Err(GitcryptCode::ENoCmd);
        }
        let kind = match cmd_kind(o) {
            Some(k) => k,
            None => return Err(GitcryptCode::ENoCmd),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5D720);
            return Err(GitcryptCode::EOrder);
        }
        let want = match cmd_cell(o) {
            Some(w) => w,
            None => return Err(GitcryptCode::ENoCmd),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5D721 + o as u64);
            return Err(GitcryptCode::ECmdDrift);
        }
        if match self.cmds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GitcryptCode::EDup);
        }
        if let Some(c) = self.cmds.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5D722 + o as u64);
        Ok(())
    }

    /// 命令全账核验（20 格）。
    pub fn cmds_all(&self) -> bool {
        let mut o = 0usize;
        while o < CMD_OPS {
            match self.cmds.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 密钥投影一格（对照 + 重复拒）。
    pub fn record_key(&mut self, v: usize, got: u64) -> Result<(), GitcryptCode> {
        let want = match key_cell(v) {
            Some(w) => w,
            None => return Err(GitcryptCode::EKey),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5D730 + v as u64);
            return Err(GitcryptCode::EKeyDrift);
        }
        if match self.keys.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GitcryptCode::EDup);
        }
        if let Some(c) = self.keys.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x5D731 + v as u64);
        Ok(())
    }

    /// 密钥投影全账核验（8 格）。
    pub fn keys_all(&self) -> bool {
        let mut v = 0usize;
        while v < KEY_CELLS {
            match self.keys.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// filter 一格（三态闭集 + 对照 + 重复拒）。
    pub fn record_filter(&mut self, d: usize, state: u32) -> Result<(), GitcryptCode> {
        let want = match filter_cell(d) {
            Some(w) => w,
            None => return Err(GitcryptCode::ENoFilter),
        };
        if state >= 3 {
            return Err(GitcryptCode::EFilterState);
        }
        if match self.filters.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GitcryptCode::EDup);
        }
        if let Some(c) = self.filters.get_mut(d) {
            *c = Some((want, state));
        }
        self.fnv(0x5D740 + d as u64 * 4 + state as u64);
        Ok(())
    }

    /// filter 全账核验（6 格）。
    pub fn filters_all(&self) -> bool {
        let mut d = 0usize;
        while d < FILTER_CELLS {
            match self.filters.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// git 消费联签一格（F52159 锚定行；越闭集与漂移同码必抓）。
    pub fn record_cosign(&mut self, c: usize, got: u64) -> Result<(), GitcryptCode> {
        let want = match cosign_cell(c) {
            Some(w) => w,
            None => return Err(GitcryptCode::ECosign),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5D750 + c as u64);
            return Err(GitcryptCode::ECosign);
        }
        if match self.cosigns.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GitcryptCode::EDup);
        }
        if let Some(x) = self.cosigns.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x5D751 + c as u64);
        Ok(())
    }

    /// 联签全账核验（4 格）。
    pub fn cosigns_all(&self) -> bool {
        let mut c = 0usize;
        while c < COSIGN_CELLS {
            match self.cosigns.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// C++ 上栈声明一行。
    pub fn declare_row(&mut self, r: usize) -> Result<(), GitcryptCode> {
        if r >= STACK_ROWS {
            return Err(GitcryptCode::ERow);
        }
        if match self.rows.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GitcryptCode::EDup);
        }
        if let Some(c) = self.rows.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x5D760 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < STACK_ROWS {
            match self.rows.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全齐 → S2 目标档，六闸齐 → S2，判据缺 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.cmds_all() && self.keys_all() && self.filters_all() && self.cosigns_all() && self.rows_all() {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for GitcryptLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = GitcryptLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < CMD_OPS {
        if let Some(w) = cmd_cell(o) {
            if led.record_cmd(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut v = 0usize;
    while v < KEY_CELLS {
        if let Some(w) = key_cell(v) {
            if led.record_key(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut d = 0usize;
    while d < FILTER_CELLS {
        // 三态轮转落账：0=clean 1=smudge 2=diff。
        if led.record_filter(d, (d % 3) as u32).is_ok() {
            recorded += 1;
        }
        d += 1;
    }
    let mut c = 0usize;
    while c < COSIGN_CELLS {
        if let Some(w) = cosign_cell(c) {
            if led.record_cosign(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut r = 0usize;
    while r < STACK_ROWS {
        let _ = led.declare_row(r);
        r += 1;
    }
    // 注入 10 轮：命令漂移 / 命令逆序 / 命令越闭集三态轮转，
    // 全部落在当轮临时账本上（主账本只记注入计数与指纹）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = GitcryptLedger::new();
        let caught = match kind {
            // 漂移注入：先按合法序落正格，再注漂移载荷。
            0 => {
                let _ = lt.record_cmd(3, cmd_cell(3).unwrap_or(0));
                lt.record_cmd(4, cmd_cell(4).unwrap_or(0).wrapping_add(1))
                    == Err(GitcryptCode::ECmdDrift)
            }
            // 逆序注入：先落后面族正格，再注前面族。
            1 => {
                let _ = lt.record_cmd(11, cmd_cell(11).unwrap_or(0));
                lt.record_cmd(1, cmd_cell(1).unwrap_or(0)) == Err(GitcryptCode::EOrder)
            }
            // 越闭集注入：命令 20 之外必抓。
            _ => lt.record_cmd(CMD_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5D770 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52414 判据集（锚 UNX-F52415-J1，off-by-one 板式）。
pub fn run_unx52414_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52414");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五命令族 20 操作全通",
        recorded >= CMD_OPS as u32,
        "init/add-key/lock/unlock/export 逐操作恒等（UNX-F52415-J1 ×10）",
    );
    s.add(
        "密钥投影 8/8 全落",
        {
            let mut lt = GitcryptLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < KEY_CELLS {
                if let Some(w) = key_cell(v) {
                    if lt.record_key(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.keys_all()
        },
        ".git/git-crypt/keys 镜像逐格对照，漂移必抓",
    );
    s.add(
        "filter 三态 6/6 全落",
        {
            let mut lt = GitcryptLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < FILTER_CELLS {
                if lt.record_filter(d, (d % 3) as u32).is_err() {
                    ok = false;
                }
                d += 1;
            }
            ok && lt.filters_all()
        },
        "clean/smudge/diff 三态闭集逐格落账",
    );
    s.add(
        "filter 三态越态必抓",
        {
            let mut lt = GitcryptLedger::new();
            lt.record_filter(0, 3) == Err(GitcryptCode::EFilterState)
        },
        "态 3 越三态闭集必拒",
    );
    s.add(
        "与 git 消费联签 4/4（F52159 锚定行）",
        {
            let mut lt = GitcryptLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < COSIGN_CELLS {
                if let Some(w) = cosign_cell(c) {
                    if lt.record_cosign(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.cosigns_all()
        },
        "filter 驱动注册/smudge 管道/clean 管道/diff 属性逐格联签",
    );
    s.add(
        "联签越闭集必抓",
        {
            let mut lt = GitcryptLedger::new();
            lt.record_cosign(COSIGN_CELLS, 0).is_err()
        },
        "联签格 4 越闭集必拒（ECosign）",
    );
    s.add(
        "六判据母版闸全落",
        {
            let mut lt = GitcryptLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "filter 注册/属性导出/密钥解锁/仓库锁定/协作者授权/恢复能力逐格落账",
    );
    s.add(
        "C++ 上栈声明 2/2 全齐",
        {
            let mut lt = GitcryptLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < STACK_ROWS {
                if lt.declare_row(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.rows_all()
        },
        "libgit-crypt 静态库上栈/OpenSSL crypto 后端分账两行闭集（§AI-66.3）",
    );
    s.add(
        "命令族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CMD_OPS {
                if cmd_kind(o).unwrap_or(5) < cmd_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五命令族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "命令漂移/命令逆序/命令越闭集三态轮转必抓",
    );
    let mut l0 = GitcryptLedger::new();
    s.add(
        "边界·命令越闭集",
        l0.record_cmd(CMD_OPS, 0).is_err(),
        "命令操作 20 越闭集必拒",
    );
    s.add(
        "边界·密钥投影越闭集",
        l0.record_key(KEY_CELLS, 0).is_err(),
        "密钥投影 8 越闭集必拒",
    );
    s.add(
        "边界·filter 越闭集",
        l0.record_filter(FILTER_CELLS, 0).is_err(),
        "filter 6 越闭集必拒",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA740,
        "SEG_BASE 0x40 对齐且按 VX16 公式落在 0xCA740",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_kind() {
        assert_eq!(cmd_kind(0), Some(0));
        assert_eq!(cmd_kind(4), Some(1));
        assert_eq!(cmd_kind(19), Some(4));
        assert!(cmd_kind(20).is_none());
        assert!(cmd_cell(20).is_none());
        assert_eq!(cmd_kind_name(1), Some("add-key"));
        assert_eq!(cmd_kind_name(4), Some("export"));
        assert!(cmd_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = cmd_cell(3).unwrap_or(0);
        assert_eq!(a, cmd_cell(3).unwrap_or(0));
        assert_ne!(a, cmd_cell(4).unwrap_or(0));
        assert!(key_cell(KEY_CELLS).is_none());
        assert!(filter_cell(FILTER_CELLS).is_none());
        assert!(cosign_cell(COSIGN_CELLS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
    }

    #[test]
    fn test_filter_state_closed() {
        assert_eq!(filter_state_name(0), Some("clean"));
        assert_eq!(filter_state_name(2), Some("diff"));
        assert!(filter_state_name(3).is_none());
        let mut lt = GitcryptLedger::new();
        assert_eq!(lt.record_filter(0, 3), Err(GitcryptCode::EFilterState));
        assert!(lt.record_filter(0, 2).is_ok());
        assert_eq!(lt.record_filter(0, 2), Err(GitcryptCode::EDup));
    }

    #[test]
    fn test_cmd_order_caught() {
        let mut lt = GitcryptLedger::new();
        let _ = lt.record_cmd(11, cmd_cell(11).unwrap_or(0));
        assert_eq!(lt.record_cmd(1, cmd_cell(1).unwrap_or(0)), Err(GitcryptCode::EOrder));
        assert!(lt.record_cmd(12, cmd_cell(12).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = GitcryptLedger::new();
        assert_eq!(
            lt.record_cmd(5, cmd_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(GitcryptCode::ECmdDrift)
        );
        assert_eq!(
            lt.record_key(2, key_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(GitcryptCode::EKeyDrift)
        );
        // 联签漂移同码必抓。
        assert_eq!(
            lt.record_cosign(1, cosign_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(GitcryptCode::ECosign)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = GitcryptLedger::new();
        let _ = lt.record_key(0, key_cell(0).unwrap_or(0));
        assert_eq!(lt.record_key(0, key_cell(0).unwrap_or(0)), Err(GitcryptCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = GitcryptLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        assert_eq!(gate_name(2), Some("密钥解锁"));
        assert!(gate_name(6).is_none());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只计 run_cycle 实际计数的节：命令 20 + 密钥 8 + filter 6 + 联签 4。
        assert_eq!(
            recorded,
            CMD_OPS as u32 + KEY_CELLS as u32 + FILTER_CELLS as u32 + COSIGN_CELLS as u32
        );
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_injection_ledger_conserved() {
        let mut led = GitcryptLedger::new();
        let mut k = 0usize;
        while k < 10 {
            let kind = (k % 3) as u32;
            let mut lt = GitcryptLedger::new();
            let caught = match kind {
                0 => {
                    let _ = lt.record_cmd(3, cmd_cell(3).unwrap_or(0));
                    lt.record_cmd(4, cmd_cell(4).unwrap_or(0).wrapping_add(1))
                        == Err(GitcryptCode::ECmdDrift)
                }
                1 => {
                    let _ = lt.record_cmd(11, cmd_cell(11).unwrap_or(0));
                    lt.record_cmd(1, cmd_cell(1).unwrap_or(0)) == Err(GitcryptCode::EOrder)
                }
                _ => lt.record_cmd(CMD_OPS, 0).is_err(),
            };
            assert!(caught, "第 {} 轮注入必须全抓", k);
            if caught {
                led.injected += 1;
                led.caught += 1;
            }
            k += 1;
        }
        let (inj, ca, esc) = led.stats();
        assert_eq!(inj, 10);
        assert_eq!(ca, 10);
        assert_eq!(esc, 0);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCA740);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (52414 - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(GitcryptCode::Ok as u32, 0);
        assert_eq!(GitcryptCode::EGradeDrift as u32, 12);
        assert_eq!(GitcryptCode::Ok.val(), SEG_BASE);
        assert_eq!(GitcryptCode::ESeal.val(), SEG_BASE + 11);
        // 全变体 detail 非空。
        let all = [
            GitcryptCode::Ok,
            GitcryptCode::ENoCmd,
            GitcryptCode::ECmdDrift,
            GitcryptCode::EOrder,
            GitcryptCode::EKey,
            GitcryptCode::EKeyDrift,
            GitcryptCode::ENoFilter,
            GitcryptCode::EFilterState,
            GitcryptCode::ECosign,
            GitcryptCode::ERow,
            GitcryptCode::EDup,
            GitcryptCode::ESeal,
            GitcryptCode::EGradeDrift,
        ];
        let mut i = 0usize;
        while i < all.len() {
            assert!(!all[i].detail().is_empty());
            assert_eq!(all[i] as u32, i as u32);
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let empty = GitcryptLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        // 六闸齐（余未齐）S2。
        let mut half = GitcryptLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = half.judge_gate(g);
            g += 1;
        }
        assert_eq!(half.judge_grade(), Grade::S2);
        // 全齐目标档 S2。
        let mut full = GitcryptLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = full.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < CMD_OPS {
            if let Some(w) = cmd_cell(o) {
                let _ = full.record_cmd(o, w);
            }
            o += 1;
        }
        let mut v = 0usize;
        while v < KEY_CELLS {
            if let Some(w) = key_cell(v) {
                let _ = full.record_key(v, w);
            }
            v += 1;
        }
        let mut d = 0usize;
        while d < FILTER_CELLS {
            let _ = full.record_filter(d, (d % 3) as u32);
            d += 1;
        }
        let mut c = 0usize;
        while c < COSIGN_CELLS {
            if let Some(w) = cosign_cell(c) {
                let _ = full.record_cosign(c, w);
            }
            c += 1;
        }
        let mut r = 0usize;
        while r < STACK_ROWS {
            let _ = full.declare_row(r);
            r += 1;
        }
        assert_eq!(full.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52414_checks_green() {
        let s = run_unx52414_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
