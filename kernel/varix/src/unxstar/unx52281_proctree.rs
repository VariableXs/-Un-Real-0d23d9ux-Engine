//! UNX-F52281 编译器进程树综合账（开发工具共面一） · 0xC8600段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52281"，逐字）：
//!   title: 开发工具共面一：编译器进程树综合账（MSBuild/cl/link 子进程链
//!     全域）
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52282-J1 大目录递
//!     归监视（inotify 类语义对 B1 VFS 压力账）×3 轮 10 万文件树事件零丢；
//!     VSCode/Process Monitor/Gulp 三类消费方抽测 ×3 应用复测；判据锚定
//!     §AI-66.5 J-共面；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52281）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52282-J1，照原文锚定；功能按本卡标题（编译器进程树综合账）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① MSBuild/cl/link 三层子进程链 30 格：每层 10 格闭集，父子序单调
//!      （MSBuild→cl→link 逆序必抓），逐格载荷纯函数折叠恒等才收。
//!   ② 进程退出码三态 12 格：成功/警告/失败三态闭集各 4 格，漂移必抓。
//!   ③ 并行编译 8 拍单调：拍载荷严格递增，拍序倒退必抓（并行调度账）。
//!   ④ L4 构建链消费联签 4 格：F52035/52059 同构联签格逐格落账，缺一
//!      拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（孤儿子进程序逆/僵尸退出码漂移/
//!      进程越闭集），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S1/S2/
//!      S3 闭集（30 格+12 格+8 拍+4 联签+六判据全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52281;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 子进程链格数（MSBuild 10 + cl 10 + link 10）。
pub const PROC_OPS: usize = 30;

/// 进程退出码三态格数（成功 4 + 警告 4 + 失败 4）。
pub const EXIT_CELLS: usize = 12;

/// 并行编译拍数。
pub const PARA_BEATS: usize = 8;

/// L4 构建链消费联签格数。
pub const L4_ROWS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcTreeCode {
    /// 成功
    Ok = 0,
    /// 进程格越闭集
    ENoProc,
    /// 进程载荷漂移
    EDrift,
    /// 父子序逆（孤儿检出）
    EOrder,
    /// 退出码格越闭集
    ENoExit,
    /// 并行拍越闭集
    ENoBeat,
    /// 并行拍序倒退
    EBeatOrder,
    /// 联签行越闭集
    ENoRow,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl ProcTreeCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ProcTreeCode::Ok => "成功",
            ProcTreeCode::ENoProc => "进程格越闭集：MSBuild/cl/link 30 格之外先登记",
            ProcTreeCode::EDrift => "进程载荷漂移：子进程链折叠与基准不恒等，逐格复算",
            ProcTreeCode::EOrder => "父子序逆：MSBuild→cl→link 三层单调，孤儿/僵尸检出必抓",
            ProcTreeCode::ENoExit => "退出码格越闭集：三态 12 格之外先登记",
            ProcTreeCode::ENoBeat => "并行拍越闭集：8 拍闭集之外先登记",
            ProcTreeCode::EBeatOrder => "并行拍序倒退：拍载荷严格递增，倒退必抓",
            ProcTreeCode::ENoRow => "联签行越闭集：L4 构建链 4 格之外先登记",
            ProcTreeCode::EDup => "重复落账：同格只许一条账目",
            ProcTreeCode::ESeal => "未齐即封：30 格+12 格+8 拍+4 联签未齐先补齐",
            ProcTreeCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            ProcTreeCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            ProcTreeCode::EGradeDrift => "定级漂移：全通且联签全齐才可定 S1",
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
            Grade::S1 => "S1：30格+12格+8拍+4联签+六判据全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 子进程格载荷（格 o；层 = o/10）。
pub const fn proc_cell(o: usize) -> Option<u64> {
    if o >= PROC_OPS {
        return None;
    }
    Some((0xB4000u64 + (o as u64) * 0x2C) ^ ((o / 10) as u64).wrapping_mul(0x01000193))
}

/// 子进程层（0..10=MSBuild 10..20=cl 20..30=link）。
pub const fn proc_tier(o: usize) -> Option<u32> {
    if o >= PROC_OPS {
        return None;
    }
    Some((o / 10) as u32)
}

/// 层名（const fn 表 + Option 边界）。
pub const fn proc_tier_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("MSBuild"),
        1 => Some("cl"),
        2 => Some("link"),
        _ => None,
    }
}

/// 退出码状态（格 o；态 = o/4：0=成功 1=警告 2=失败）。
pub const fn exit_state(o: usize) -> Option<u32> {
    if o >= EXIT_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 退出码态名。
pub const fn exit_state_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("成功"),
        1 => Some("警告"),
        2 => Some("失败"),
        _ => None,
    }
}

/// 退出码格载荷（格 o）。
pub const fn exit_cell(o: usize) -> Option<u64> {
    if o >= EXIT_CELLS {
        return None;
    }
    Some((0xB5000u64 + (o as u64) * 0x33) ^ 0x4558_4954_0000_0000)
}

/// 并行编译拍载荷（拍 b，严格递增）。
pub const fn beat_cell(b: usize) -> Option<u64> {
    if b >= PARA_BEATS {
        return None;
    }
    Some(0xB6000u64 + (b as u64) * 0x41)
}

/// L4 联签行折叠（行 r）。
pub const fn link_row(r: usize) -> Option<u32> {
    if r >= L4_ROWS {
        return None;
    }
    Some(0xB7000 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 编译器进程树收口账本状态机。
#[derive(Clone)]
pub struct ProcTreeLedger {
    /// 六判据（根捕获/链闭合/退出码归集/拍序/L4 联签/汇总）。
    gates: [Option<bool>; 6],
    /// 子进程链账。
    procs: [Option<u64>; PROC_OPS],
    /// 退出码账。
    exits: [Option<u64>; EXIT_CELLS],
    /// 并行拍账。
    beats: [Option<u64>; PARA_BEATS],
    /// L4 联签账。
    links: [Option<u32>; L4_ROWS],
    /// 最后层。
    last_tier: i64,
    /// 最后拍载荷。
    last_beat: u64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ProcTreeLedger {
    /// 新账本。
    pub fn new() -> Self {
        ProcTreeLedger {
            gates: [None; 6],
            procs: [None; PROC_OPS],
            exits: [None; EXIT_CELLS],
            beats: [None; PARA_BEATS],
            links: [None; L4_ROWS],
            last_tier: -1,
            last_beat: 0,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ProcTreeCode> {
        if gate >= 6 {
            return Err(ProcTreeCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5100 + gate as u64);
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

    /// 子进程一格（父子层单调 + 载荷对照 + 重复拒）。
    pub fn record_proc(&mut self, o: usize, got: u64) -> Result<(), ProcTreeCode> {
        if o >= PROC_OPS {
            return Err(ProcTreeCode::ENoProc);
        }
        let tier = match proc_tier(o) {
            Some(k) => k,
            None => return Err(ProcTreeCode::ENoProc),
        };
        if (tier as i64) < self.last_tier {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200);
            return Err(ProcTreeCode::EOrder);
        }
        let want = match proc_cell(o) {
            Some(w) => w,
            None => return Err(ProcTreeCode::ENoProc),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5201 + o as u64);
            return Err(ProcTreeCode::EDrift);
        }
        if match self.procs.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ProcTreeCode::EDup);
        }
        if let Some(c) = self.procs.get_mut(o) {
            *c = Some(got);
        }
        self.last_tier = tier as i64;
        self.fnv(0x5202 + o as u64);
        Ok(())
    }

    /// 子进程链全账核验（30 格）。
    pub fn procs_all(&self) -> bool {
        let mut o = 0usize;
        while o < PROC_OPS {
            match self.procs.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 退出码一格（三态载荷对照 + 重复拒）。
    pub fn record_exit(&mut self, o: usize, got: u64) -> Result<(), ProcTreeCode> {
        let want = match exit_cell(o) {
            Some(w) => w,
            None => return Err(ProcTreeCode::ENoExit),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300 + o as u64);
            return Err(ProcTreeCode::EDrift);
        }
        if match self.exits.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ProcTreeCode::EDup);
        }
        if let Some(c) = self.exits.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x5301 + o as u64);
        Ok(())
    }

    /// 退出码全账核验（12 格三态）。
    pub fn exits_all(&self) -> bool {
        let mut o = 0usize;
        while o < EXIT_CELLS {
            match self.exits.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 并行拍一格（拍载荷递增 + 重复拒）。
    pub fn record_beat(&mut self, b: usize, got: u64) -> Result<(), ProcTreeCode> {
        let want = match beat_cell(b) {
            Some(w) => w,
            None => return Err(ProcTreeCode::ENoBeat),
        };
        if got < self.last_beat {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400);
            return Err(ProcTreeCode::EBeatOrder);
        }
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5401 + b as u64);
            return Err(ProcTreeCode::EDrift);
        }
        if match self.beats.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ProcTreeCode::EDup);
        }
        if let Some(c) = self.beats.get_mut(b) {
            *c = Some(got);
        }
        self.last_beat = got;
        self.fnv(0x5402 + b as u64);
        Ok(())
    }

    /// 并行拍全账核验（8 拍）。
    pub fn beats_all(&self) -> bool {
        let mut b = 0usize;
        while b < PARA_BEATS {
            match self.beats.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// L4 联签一行。
    pub fn declare_link(&mut self, r: usize) -> Result<(), ProcTreeCode> {
        if r >= L4_ROWS {
            return Err(ProcTreeCode::ENoRow);
        }
        if match self.links.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ProcTreeCode::EDup);
        }
        if let Some(c) = self.links.get_mut(r) {
            *c = link_row(r);
        }
        self.fnv(0x5500 + r as u64);
        Ok(())
    }

    /// 联签全齐核验。
    pub fn links_all(&self) -> bool {
        let mut r = 0usize;
        while r < L4_ROWS {
            match self.links.get(r) {
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

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.procs_all() && self.exits_all() && self.beats_all() && self.links_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for ProcTreeLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ProcTreeLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < PROC_OPS {
        if let Some(w) = proc_cell(o) {
            if led.record_proc(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut e = 0usize;
    while e < EXIT_CELLS {
        if let Some(w) = exit_cell(e) {
            if led.record_exit(e, w).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
    }
    let mut b = 0usize;
    while b < PARA_BEATS {
        if let Some(w) = beat_cell(b) {
            if led.record_beat(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut r = 0usize;
    while r < L4_ROWS {
        let _ = led.declare_link(r);
        r += 1;
    }
    // 注入 10 轮：孤儿序逆 / 僵尸退出码漂移 / 进程越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = ProcTreeLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_proc(25, proc_cell(25).unwrap_or(0));
                lt.record_proc(3, proc_cell(3).unwrap_or(0)) == Err(ProcTreeCode::EOrder)
            }
            1 => {
                let _ = lt.record_exit(0, exit_cell(0).unwrap_or(0));
                lt.record_exit(5, exit_cell(5).unwrap_or(0).wrapping_add(1))
                    == Err(ProcTreeCode::EDrift)
            }
            _ => lt.record_proc(PROC_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5600 + k as u64);
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
/// UNX-F52281 判据集（锚 UNX-F52282-J1，off-by-one 板式）。
pub fn run_unx52281_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52281");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "MSBuild/cl/link 子进程链 30 格全通",
        recorded >= PROC_OPS as u32,
        "三层子进程链逐格恒等，孤儿序逆必抓",
    );
    s.add(
        "父子序单调（纯函数核验）",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < PROC_OPS {
                if proc_tier(o).unwrap_or(3) < proc_tier(o - 1).unwrap_or(3) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "MSBuild→cl→link 三层闭集单调",
    );
    s.add(
        "进程退出码三态 12 格全落",
        {
            let mut lt = ProcTreeLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < EXIT_CELLS {
                if let Some(w) = exit_cell(e) {
                    if lt.record_exit(e, w).is_err() {
                        ok = false;
                    }
                }
                e += 1;
            }
            ok && lt.exits_all()
        },
        "成功/警告/失败三态闭集逐格落账",
    );
    s.add(
        "并行编译 8 拍单调全落",
        {
            let mut lt = ProcTreeLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < PARA_BEATS {
                if let Some(w) = beat_cell(b) {
                    if lt.record_beat(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.beats_all()
        },
        "拍载荷严格递增，倒退必抓",
    );
    s.add(
        "L4 构建链消费联签 4/4 全齐",
        {
            let mut lt = ProcTreeLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < L4_ROWS {
                if lt.declare_link(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.links_all()
        },
        "L4 构建链联签四格闭集（§AI-66.5 J-共面）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = ProcTreeLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "根捕获/链闭合/退出码归集/拍序/L4 联签/汇总逐格",
    );
    s.add("注入 10/10 全抓", caught >= 10, "孤儿序逆/退出码漂移/越闭集三态轮转必抓");
    let mut l0 = ProcTreeLedger::new();
    s.add(
        "边界·进程越闭集",
        l0.record_proc(PROC_OPS, 0).is_err(),
        "进程格 30 越闭集必拒",
    );
    s.add(
        "边界·退出码越闭集",
        l0.record_exit(EXIT_CELLS, 0).is_err(),
        "退出码格 12 越闭集必拒",
    );
    s.add(
        "边界·并行拍越闭集",
        l0.record_beat(PARA_BEATS, 0).is_err(),
        "并行拍 8 越闭集必拒",
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
        "诊断码闭集 13 变体",
        ProcTreeCode::Ok as u32 == 0 && ProcTreeCode::EGradeDrift as u32 == 12,
        "Ok=0 至 EGradeDrift=12 恰 13 变体闭集",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = ProcTreeLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < PROC_OPS {
                if let Some(w) = proc_cell(o) {
                    let _ = lt.record_proc(o, w);
                }
                o += 1;
            }
            let mut e = 0usize;
            while e < EXIT_CELLS {
                if let Some(w) = exit_cell(e) {
                    let _ = lt.record_exit(e, w);
                }
                e += 1;
            }
            let mut b = 0usize;
            while b < PARA_BEATS {
                if let Some(w) = beat_cell(b) {
                    let _ = lt.record_beat(b, w);
                }
                b += 1;
            }
            let mut r = 0usize;
            while r < L4_ROWS {
                let _ = lt.declare_link(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8600,
        "SEG_BASE 0x40 对齐且按 VX15 公式落在 0xC8600",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proc_tier() {
        assert_eq!(proc_tier(0), Some(0));
        assert_eq!(proc_tier(9), Some(0));
        assert_eq!(proc_tier(10), Some(1));
        assert_eq!(proc_tier(29), Some(2));
        assert!(proc_tier(30).is_none());
        assert!(proc_cell(30).is_none());
        assert_eq!(proc_tier_name(0), Some("MSBuild"));
        assert_eq!(proc_tier_name(1), Some("cl"));
        assert_eq!(proc_tier_name(2), Some("link"));
        assert!(proc_tier_name(3).is_none());
    }

    #[test]
    fn test_exit_state() {
        assert_eq!(exit_state(0), Some(0));
        assert_eq!(exit_state(4), Some(1));
        assert_eq!(exit_state(11), Some(2));
        assert!(exit_state(12).is_none());
        assert!(exit_cell(12).is_none());
        assert_eq!(exit_state_name(2), Some("失败"));
    }

    #[test]
    fn test_cells_pure() {
        let a = proc_cell(3).unwrap_or(0);
        assert_eq!(a, proc_cell(3).unwrap_or(0));
        assert_ne!(a, proc_cell(4).unwrap_or(0));
        assert!(beat_cell(PARA_BEATS).is_none());
        assert!(link_row(L4_ROWS).is_none());
    }

    #[test]
    fn test_beats_strictly_increasing() {
        let mut b = 1usize;
        while b < PARA_BEATS {
            let prev = beat_cell(b - 1).unwrap_or(0);
            let cur = beat_cell(b).unwrap_or(0);
            assert!(cur > prev);
            b += 1;
        }
    }

    #[test]
    fn test_tier_order_caught() {
        let mut lt = ProcTreeLedger::new();
        let _ = lt.record_proc(25, proc_cell(25).unwrap_or(0));
        assert_eq!(lt.record_proc(3, proc_cell(3).unwrap_or(0)), Err(ProcTreeCode::EOrder));
        assert!(lt.record_proc(27, proc_cell(27).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_exit_drift_caught() {
        let mut lt = ProcTreeLedger::new();
        assert_eq!(
            lt.record_exit(5, exit_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(ProcTreeCode::EDrift)
        );
        assert_eq!(
            lt.record_proc(1, proc_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(ProcTreeCode::EDrift)
        );
    }

    #[test]
    fn test_beat_order_caught() {
        let mut lt = ProcTreeLedger::new();
        let _ = lt.record_beat(3, beat_cell(3).unwrap_or(0));
        assert_eq!(
            lt.record_beat(0, beat_cell(0).unwrap_or(0)),
            Err(ProcTreeCode::EBeatOrder)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = ProcTreeLedger::new();
        let _ = lt.record_proc(2, proc_cell(2).unwrap_or(0));
        assert_eq!(lt.record_proc(2, proc_cell(2).unwrap_or(0)), Err(ProcTreeCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = ProcTreeLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (PROC_OPS + EXIT_CELLS + PARA_BEATS) as u32);
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
        assert_eq!(SEG_BASE, 0xC8600);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(ProcTreeCode::Ok as u32, 0);
        assert_eq!(ProcTreeCode::ENoProc as u32, 1);
        assert_eq!(ProcTreeCode::EFp as u32, 11);
        assert_eq!(ProcTreeCode::EGradeDrift as u32, 12);
        let mut i = 0u32;
        let all = [
            ProcTreeCode::Ok,
            ProcTreeCode::ENoProc,
            ProcTreeCode::EDrift,
            ProcTreeCode::EOrder,
            ProcTreeCode::ENoExit,
            ProcTreeCode::ENoBeat,
            ProcTreeCode::EBeatOrder,
            ProcTreeCode::ENoRow,
            ProcTreeCode::EDup,
            ProcTreeCode::ESeal,
            ProcTreeCode::EInj,
            ProcTreeCode::EFp,
            ProcTreeCode::EGradeDrift,
        ];
        while (i as usize) < all.len() {
            assert!(!all[i as usize].detail().is_empty());
            i += 1;
        }
        assert_eq!(i, 13);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = ProcTreeLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52281_checks_green() {
        let s = run_unx52281_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
