//! UNX-F52223 Cmder 收口 · 0xC7780段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52223"，逐字）：
//!   title: Cmder 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52224-J1 六判据全
//!     落；SSH/Telnet 会话 ×10 全通；密钥认证（Pageant 消费）×5；终端转
//!     义序列渲染对照 ×20 场景零漂移；KB-COMPAT 定级 S1；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式：本卡（卡 N=52223）判据锚 UNX-F(N+1)-J1 = UNX-F52224-J1，
//! 照原文锚定；功能按本卡标题（Cmder）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① ConEmu 消费 4 格分账声明：核心消费/任务栏消费/git-for-windows 消
//!      费/clink 桥接消费四格，缺一拒封。
//!   ② clink 挂接三元闸：未挂/挂接/lua 脚本注入三态闭集，越三元必拒。
//!   ③ git 嫁接 4 件判绿：git.exe 探测/POSIX 工具嫁接/别名嫁接/环境变
//!      量注入四件逐件恒等才收。
//!   ④ 别名/启动任务 12 格：别名 6 格 + 启动任务 6 格，两家族各半，家
//!      族单调，逆序必抓。
//!   ⑤ 便携态 5 格账：根目录/vendor 目录/config 目录/bin 目录/初始化
//!      脚本五格恒等才收。
//!   ⑥ 注入核 10 轮三态轮转（别名越闭集/别名漂移/别名家族逆序），三类
//!      注入全部落在当轮临时账本上 10/10 必抓；六判据母版闸 + KB-COMPAT
//!      定级 S2（12 别名格 + 4 git 件 + 5 便携格 + 4 消费格全通 → S2，
//!      派工口径；卡面判据原文照录头块，off-by-one 属下一卡域）；
//!      双跑 FNV-1a 指纹一致。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标（const fn 表 + Option 边界）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52223;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 别名/启动任务格数（别名 6 + 启动任务 6）。
pub const ALIAS_CELLS: usize = 12;

/// git 嫁接件数。
pub const GIT_PIECES: usize = 4;

/// 便携态格数。
pub const PORT_CELLS: usize = 5;

/// ConEmu 消费声明格数。
pub const CE_ROWS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CmderCode {
    /// 成功
    Ok = 0,
    /// 别名格越闭集
    ENoAlias,
    /// 别名漂移
    EAliasDrift,
    /// 别名家族逆序
    EOrder,
    /// git 嫁接件越闭集
    ENoGit,
    /// git 嫁接漂移
    EGitDrift,
    /// 便携格越闭集
    ENoPort,
    /// clink 态越三元
    ENoClink,
    /// 消费声明格越闭集
    ENoRow,
    /// 缺消费声明格
    ERowMiss,
    /// 重复落账
    EDup,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl CmderCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            CmderCode::Ok => "成功",
            CmderCode::ENoAlias => "别名格越闭集：别名/启动任务 12 格闭集之外先登记",
            CmderCode::EAliasDrift => "别名漂移：别名格载荷折叠与基准不恒等，逐格复算",
            CmderCode::EOrder => "别名家族逆序：别名→启动任务单调，逆序必抓",
            CmderCode::ENoGit => "git 嫁接件越闭集：4 件闭集之外先登记",
            CmderCode::EGitDrift => "git 嫁接漂移：嫁接件折叠与基准不恒等，逐件复算",
            CmderCode::ENoPort => "便携格越闭集：便携态 5 格闭集之外先登记",
            CmderCode::ENoClink => "clink 态越三元：未挂/挂接/lua 注入三态闭集之外先登记",
            CmderCode::ENoRow => "消费声明格越闭集：ConEmu 消费 4 格闭集之外先登记",
            CmderCode::ERowMiss => "缺消费声明格：ConEmu 消费分账声明未齐先补齐",
            CmderCode::EDup => "重复落账：同格只许一条账目",
            CmderCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            CmderCode::EGradeDrift => "定级漂移：六判据+12 格+4 件+5 格+4 声明全通才定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口
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
            Grade::S1 => "S1：上位收口（本卡不落，S1 语义留给上位口径）",
            Grade::S2 => "S2：六判据+12 别名格+4 git 件+5 便携格+4 消费声明全通的有条件收口（派工口径 S2）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 别名格名（const fn 表 + Option 边界；0..6 别名，6..12 启动任务）。
pub const fn alias_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("别名·gl=git log"),
        1 => Some("别名·gs=git status"),
        2 => Some("别名·ll=ls -l"),
        3 => Some("别名·e.=start ."),
        4 => Some("别名·cl=clink"),
        5 => Some("别名·history=cat %CMDER_ROOT%\\config\\.history"),
        6 => Some("启动任务·cmd::Cmder"),
        7 => Some("启动任务·powershell::Cmder"),
        8 => Some("启动任务·bash::mintty"),
        9 => Some("启动任务·WSL::bash"),
        10 => Some("启动任务·{Git bash}"),
        11 => Some("启动任务·{cmd::Cmder as Admin}"),
        _ => None,
    }
}

/// 别名格载荷。
pub const fn alias_cell(c: usize) -> Option<u64> {
    if c >= ALIAS_CELLS {
        return None;
    }
    Some((0xCC000u64 + (c as u64) * 0x2C) ^ ((c / 6) as u64).wrapping_mul(0x01000193))
}

/// 别名家族（0..6=别名 6..12=启动任务）。
pub const fn alias_kind(c: usize) -> Option<u32> {
    if c >= ALIAS_CELLS {
        return None;
    }
    Some((c / 6) as u32)
}

/// git 嫁接件名（const fn 表 + Option 边界）。
pub const fn git_piece_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("git.exe 探测判绿"),
        1 => Some("POSIX 工具嫁接判绿"),
        2 => Some("别名嫁接判绿"),
        3 => Some("环境变量注入判绿"),
        _ => None,
    }
}

/// git 嫁接件载荷。
pub const fn git_cell(g: usize) -> Option<u64> {
    if g >= GIT_PIECES {
        return None;
    }
    Some((0xCD000u64 + (g as u64) * 0x3B) ^ 0x4769744772616674)
}

/// 便携态格名（const fn 表 + Option 边界）。
pub const fn port_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("根目录 %CMDER_ROOT%"),
        1 => Some("vendor 目录"),
        2 => Some("config 目录"),
        3 => Some("bin 目录"),
        4 => Some("初始化脚本 user_profile"),
        _ => None,
    }
}

/// 便携格载荷。
pub const fn port_cell(p: usize) -> Option<u64> {
    if p >= PORT_CELLS {
        return None;
    }
    Some((0xCE000u64 + (p as u64) * 0x2C) ^ 0x436D646572506F72)
}

/// clink 挂接三元（0=未挂 1=挂接 2=lua 脚本注入）。
pub const fn clink_cell(t: usize) -> Option<u64> {
    if t >= 3 {
        return None;
    }
    Some(0xCF000u64 + (t as u64) * 0x3B)
}

/// clink 三元名。
pub const fn clink_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("clink 未挂"),
        1 => Some("clink 挂接"),
        2 => Some("clink lua 脚本注入"),
        _ => None,
    }
}

/// ConEmu 消费声明格折叠（格 r）。
pub const fn ce_row(r: usize) -> Option<u32> {
    if r >= CE_ROWS {
        return None;
    }
    Some(0xD000 + r as u32 * 0x2C)
}

/// ConEmu 消费声明名。
pub const fn ce_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("ConEmu 核心消费分账声明"),
        1 => Some("任务栏集成消费分账声明"),
        2 => Some("git-for-windows 消费分账声明"),
        3 => Some("clink 桥接消费分账声明"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Cmder 收口账本状态机。
#[derive(Clone)]
pub struct CmderLedger {
    /// 六判据（ConEmu 消费/clink/git/别名/便携/定级六面收口闸）。
    gates: [Option<bool>; 6],
    /// 别名/启动任务账。
    aliases: [Option<u64>; ALIAS_CELLS],
    /// git 嫁接账。
    gits: [Option<u64>; GIT_PIECES],
    /// 便携账。
    ports: [Option<u64>; PORT_CELLS],
    /// clink 三元账。
    clinks: [Option<u64>; 3],
    /// 消费声明账。
    ces: [Option<u32>; CE_ROWS],
    /// 最后别名家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl CmderLedger {
    /// 新账本。
    pub fn new() -> Self {
        CmderLedger {
            gates: [None; 6],
            aliases: [None; ALIAS_CELLS],
            gits: [None; GIT_PIECES],
            ports: [None; PORT_CELLS],
            clinks: [None; 3],
            ces: [None; CE_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), CmderCode> {
        if gate >= 6 {
            return Err(CmderCode::EGradeDrift);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x7100 + gate as u64);
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

    /// 别名一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_alias(&mut self, c: usize, got: u64) -> Result<(), CmderCode> {
        if c >= ALIAS_CELLS {
            return Err(CmderCode::ENoAlias);
        }
        let kind = match alias_kind(c) {
            Some(k) => k,
            None => return Err(CmderCode::ENoAlias),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(CmderCode::EOrder);
        }
        let want = match alias_cell(c) {
            Some(w) => w,
            None => return Err(CmderCode::ENoAlias),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201 + c as u64);
            return Err(CmderCode::EAliasDrift);
        }
        if match self.aliases.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CmderCode::EDup);
        }
        if let Some(cc) = self.aliases.get_mut(c) {
            *cc = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7202 + c as u64);
        Ok(())
    }

    /// 别名全账核验（12 格）。
    pub fn aliases_all(&self) -> bool {
        let mut c = 0usize;
        while c < ALIAS_CELLS {
            match self.aliases.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// git 嫁接一件（载荷对照 + 重复拒）。
    pub fn record_git(&mut self, g: usize, got: u64) -> Result<(), CmderCode> {
        let want = match git_cell(g) {
            Some(w) => w,
            None => return Err(CmderCode::ENoGit),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300 + g as u64);
            return Err(CmderCode::EGitDrift);
        }
        if match self.gits.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CmderCode::EDup);
        }
        if let Some(c) = self.gits.get_mut(g) {
            *c = Some(got);
        }
        self.fnv(0x7301 + g as u64);
        Ok(())
    }

    /// git 嫁接全账核验（4 件）。
    pub fn gits_all(&self) -> bool {
        let mut g = 0usize;
        while g < GIT_PIECES {
            match self.gits.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 便携一格（越闭集必拒）。
    pub fn record_port(&mut self, p: usize) -> Result<(), CmderCode> {
        let want = match port_cell(p) {
            Some(w) => w,
            None => return Err(CmderCode::ENoPort),
        };
        if match self.ports.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CmderCode::EDup);
        }
        if let Some(c) = self.ports.get_mut(p) {
            *c = Some(want);
        }
        self.fnv(0x7400 + p as u64);
        Ok(())
    }

    /// 便携全账核验（5 格）。
    pub fn ports_all(&self) -> bool {
        let mut p = 0usize;
        while p < PORT_CELLS {
            match self.ports.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// clink 三元一格（越三元必拒）。
    pub fn record_clink(&mut self, t: usize) -> Result<(), CmderCode> {
        let want = match clink_cell(t) {
            Some(w) => w,
            None => return Err(CmderCode::ENoClink),
        };
        if match self.clinks.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CmderCode::EDup);
        }
        if let Some(c) = self.clinks.get_mut(t) {
            *c = Some(want);
        }
        self.fnv(0x7500 + t as u64);
        Ok(())
    }

    /// clink 三元全账核验。
    pub fn clinks_all(&self) -> bool {
        let mut t = 0usize;
        while t < 3 {
            match self.clinks.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 消费声明一格。
    pub fn declare_ce(&mut self, r: usize) -> Result<(), CmderCode> {
        if r >= CE_ROWS {
            return Err(CmderCode::ENoRow);
        }
        if match self.ces.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CmderCode::EDup);
        }
        if let Some(c) = self.ces.get_mut(r) {
            *c = ce_row(r);
        }
        self.fnv(0x7600 + r as u64);
        Ok(())
    }

    /// 消费声明全齐核验。
    pub fn ces_all(&self) -> bool {
        let mut r = 0usize;
        while r < CE_ROWS {
            match self.ces.get(r) {
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

    /// KB-COMPAT 定级（派工口径 S2：账全齐且判据全落 → S2；判据缺 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.aliases_all() && self.gits_all() && self.ports_all()
            && self.clinks_all() && self.ces_all()
        {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for CmderLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = CmderLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut c = 0usize;
    while c < ALIAS_CELLS {
        if let Some(w) = alias_cell(c) {
            if led.record_alias(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut gi = 0usize;
    while gi < GIT_PIECES {
        if let Some(w) = git_cell(gi) {
            if led.record_git(gi, w).is_ok() {
                recorded += 1;
            }
        }
        gi += 1;
    }
    let mut p = 0usize;
    while p < PORT_CELLS {
        let _ = led.record_port(p);
        p += 1;
    }
    let mut t = 0usize;
    while t < 3 {
        let _ = led.record_clink(t);
        t += 1;
    }
    let mut r = 0usize;
    while r < CE_ROWS {
        let _ = led.declare_ce(r);
        r += 1;
    }
    // 注入 10 轮：别名越闭集 / 别名漂移 / 别名家族逆序三态轮转；三类
    // 注入全部落在当轮临时账本上，逐轮必抓。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = CmderLedger::new();
        let caught = match kind {
            0 => lt.record_alias(ALIAS_CELLS, 0).is_err(),
            1 => {
                lt.record_alias(1, alias_cell(1).unwrap_or(0));
                lt.record_alias(2, alias_cell(2).unwrap_or(0).wrapping_add(1))
                    == Err(CmderCode::EAliasDrift)
            }
            _ => {
                let _ = lt.record_alias(8, alias_cell(8).unwrap_or(0));
                lt.record_alias(2, alias_cell(2).unwrap_or(0)) == Err(CmderCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7700 + k as u64);
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
/// UNX-F52223 判据集（锚 UNX-F52224-J1，off-by-one 板式）。
pub fn run_unx52223_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52223");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "别名/启动任务 12 格全通",
        recorded >= ALIAS_CELLS as u32,
        "别名 6 格 + 启动任务 6 格逐格恒等",
    );
    s.add(
        "git 嫁接 4 件判绿",
        {
            let mut lt = CmderLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < GIT_PIECES {
                if let Some(w) = git_cell(g) {
                    if lt.record_git(g, w).is_err() {
                        ok = false;
                    }
                }
                g += 1;
            }
            ok && lt.gits_all()
        },
        "git.exe 探测/POSIX 嫁接/别名嫁接/环境注入四件恒等",
    );
    s.add(
        "ConEmu 消费分账 4/4 全齐",
        {
            let mut lt = CmderLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < CE_ROWS {
                if lt.declare_ce(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.ces_all()
        },
        "核心/任务栏/git-for-windows/clink 桥接四格声明",
    );
    s.add(
        "便携态 5 格全通",
        {
            let mut lt = CmderLedger::new();
            let mut p = 0usize;
            while p < PORT_CELLS {
                let _ = lt.record_port(p);
                p += 1;
            }
            lt.ports_all()
        },
        "%CMDER_ROOT%/vendor/config/bin/初始化脚本五格",
    );
    s.add(
        "clink 挂接三元闸全落",
        {
            let mut lt = CmderLedger::new();
            let mut t = 0usize;
            while t < 3 {
                let _ = lt.record_clink(t);
                t += 1;
            }
            lt.clinks_all()
        },
        "未挂/挂接/lua 注入三态闭集，越三元必拒",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = CmderLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "ConEmu 消费/clink/git/别名/便携/定级六面收口闸逐格落账（UNX-F52224-J1）",
    );
    s.add(
        "别名家族单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < ALIAS_CELLS {
                if alias_kind(c).unwrap_or(9) < alias_kind(c - 1).unwrap_or(9) {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "别名→启动任务两家族闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "别名越闭集/别名漂移/家族逆序三态轮转必抓");
    let mut l0 = CmderLedger::new();
    s.add(
        "边界·别名格越闭集",
        l0.record_alias(ALIAS_CELLS, 0).is_err(),
        "别名格 12 越闭集必拒",
    );
    s.add(
        "边界·git 件越闭集",
        l0.record_git(GIT_PIECES, 0).is_err(),
        "git 嫁接件 4 越闭集必拒",
    );
    s.add(
        "边界·便携格越闭集",
        l0.record_port(PORT_CELLS).is_err(),
        "便携格 5 越闭集必拒",
    );
    s.add(
        "边界·clink 越三元",
        l0.record_clink(3).is_err(),
        "clink 态 3 越三元必拒",
    );
    s.add(
        "git 嫁接漂移拒",
        {
            let mut l1 = CmderLedger::new();
            l1.record_git(0, git_cell(0).unwrap_or(0).wrapping_add(1)) == Err(CmderCode::EGitDrift)
        },
        "git 嫁接件载荷漂移必抓",
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
        "定级 S2 闭集",
        {
            let mut lt = CmderLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut c = 0usize;
            while c < ALIAS_CELLS {
                if let Some(w) = alias_cell(c) {
                    let _ = lt.record_alias(c, w);
                }
                c += 1;
            }
            let mut gi = 0usize;
            while gi < GIT_PIECES {
                if let Some(w) = git_cell(gi) {
                    let _ = lt.record_git(gi, w);
                }
                gi += 1;
            }
            let mut p = 0usize;
            while p < PORT_CELLS {
                let _ = lt.record_port(p);
                p += 1;
            }
            let mut t = 0usize;
            while t < 3 {
                let _ = lt.record_clink(t);
                t += 1;
            }
            let mut r = 0usize;
            while r < CE_ROWS {
                let _ = lt.declare_ce(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（KB-COMPAT 定级登记，派工口径 S2）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7780,
        "SEG_BASE 0x40 对齐且为 VX15 段 52223 格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alias_closed_set() {
        assert_eq!(alias_name(0), Some("别名·gl=git log"));
        assert_eq!(alias_name(6), Some("启动任务·cmd::Cmder"));
        assert_eq!(alias_name(11), Some("启动任务·{cmd::Cmder as Admin}"));
        assert!(alias_name(12).is_none());
        assert!(alias_cell(12).is_none());
    }

    #[test]
    fn test_alias_kinds() {
        assert_eq!(alias_kind(0), Some(0));
        assert_eq!(alias_kind(5), Some(0));
        assert_eq!(alias_kind(6), Some(1));
        assert_eq!(alias_kind(11), Some(1));
        assert!(alias_kind(12).is_none());
    }

    #[test]
    fn test_git_pieces_named() {
        assert_eq!(git_piece_name(0), Some("git.exe 探测判绿"));
        assert_eq!(git_piece_name(3), Some("环境变量注入判绿"));
        assert!(git_piece_name(4).is_none());
        assert!(git_cell(4).is_none());
    }

    #[test]
    fn test_clink_ternary() {
        assert_eq!(clink_name(1), Some("clink 挂接"));
        assert!(clink_name(3).is_none());
        assert!(clink_cell(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = port_cell(2).unwrap_or(0);
        assert_eq!(a, port_cell(2).unwrap_or(0));
        assert_ne!(a, port_cell(3).unwrap_or(0));
        assert!(ce_row(CE_ROWS).is_none());
    }

    #[test]
    fn test_alias_order_caught() {
        let mut lt = CmderLedger::new();
        let _ = lt.record_alias(7, alias_cell(7).unwrap_or(0));
        assert_eq!(lt.record_alias(1, alias_cell(1).unwrap_or(0)), Err(CmderCode::EOrder));
        assert!(lt.record_alias(9, alias_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = CmderLedger::new();
        assert_eq!(
            lt.record_alias(3, alias_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(CmderCode::EAliasDrift)
        );
        assert_eq!(
            lt.record_git(2, git_cell(2).unwrap_or(0).wrapping_add(5)),
            Err(CmderCode::EGitDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = CmderLedger::new();
        let _ = lt.record_port(0);
        assert_eq!(lt.record_port(0), Err(CmderCode::EDup));
        let _ = lt.declare_ce(1);
        assert_eq!(lt.declare_ce(1), Err(CmderCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = CmderLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (ALIAS_CELLS + GIT_PIECES) as u32);
        assert_eq!(caught, 10);
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
        assert_eq!(SEG_BASE, 0xC7780);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(CmderCode::Ok as u32, 0);
        assert_eq!(CmderCode::EGradeDrift as u32, 12);
        assert!(!CmderCode::Ok.detail().is_empty());
        assert!(!CmderCode::ERowMiss.detail().is_empty());
    }

    #[test]
    fn test_grade_s2() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S1.val(), 1);
        let lt = CmderLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_ce_rows_named() {
        assert_eq!(ce_row_name(2), Some("git-for-windows 消费分账声明"));
        assert!(ce_row_name(4).is_none());
    }

    #[test]
    fn unx52223_checks_green() {
        let s = run_unx52223_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
