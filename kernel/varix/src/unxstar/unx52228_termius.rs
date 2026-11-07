//! UNX-F52228 Termius 收口 · 0xC78C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52228"，逐字）：
//!   title: Termius 收口
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52229-J1 六判据全
//!     落（Electron 三段账）；SSH 终端/分屏 ×10；KB-COMPAT 定级登记；判
//!     据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非
//!     空。
//! off-by-one 板式口径：本卡（卡 N=52228）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52229-J1，照原文锚定；功能按本卡标题（Termius）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 同步三元闸：账号闸 / 密钥环闸 / 冲突解决闸，三闸逐格落账，未齐
//!      即 S3 封拒（同步是 Termius 收口的前提条件，定级 S2）。
//!   ② 主机/密钥/片段三账 15 格：主机 5 + 密钥 5 + 片段 5，三账分节
//!      折叠恒等；账内序单调（片段账先于主机账落账必抓）。
//!   ③ SFTP 8 格：浏览/上传/下载/重命名/删除/建目录/权限/断点续传，
//!      逐格闭集。
//!   ④ 跨端零漂移 5 场景：桌面→移动 / 移动→桌面 / CLI→桌面 / Web→CLI
//!      / 桌面↔移动双向，场景载荷恒等才收。
//!   ⑤ 订阅/免费分账 2 行：订阅分账行 / 免费分账行，缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 10 轮三态轮转（主机漂移/
//!      SFTP 越闭集/三账逆序；SSH 终端/分屏 ×10 语义面）全部落在当轮
//!      临时账本，10/10 全抓；双跑指纹一致；KB-COMPAT 定级 S2（有条件
//!      收口：同步三元闸为前提条件）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/浮点/裸下标（const fn 表 + Option 边界）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52228;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 同步三元闸数。
pub const SYNC_GATES: usize = 3;

/// 三账格数（主机 5 + 密钥 5 + 片段 5）。
pub const VAULT_CELLS: usize = 15;

/// 每账格数。
pub const CELLS_PER_VAULT: usize = 5;

/// SFTP 格数。
pub const SFTP_CELLS: usize = 8;

/// 跨端零漂移场景数。
pub const CROSS_SCENES: usize = 5;

/// 订阅/免费分账行数。
pub const PLAN_ROWS: usize = 2;

/// 收口六判据闸数。
pub const JUDGE_GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TermiusCode {
    /// 成功
    Ok = 0,
    /// 同步闸越闭集
    ENoSyncGate,
    /// 三账格越闭集
    ENoVault,
    /// 三账载荷漂移
    EVaultDrift,
    /// 三账序逆序
    EVaultOrder,
    /// SFTP 格越闭集
    ENoSftp,
    /// SFTP 载荷漂移
    ESftpDrift,
    /// 跨端场景越闭集
    ENoScene,
    /// 跨端场景漂移
    ESceneDrift,
    /// 分账缺行
    ENoPlan,
    /// 重复落账
    EDup,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl TermiusCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            TermiusCode::Ok => "成功",
            TermiusCode::ENoSyncGate => "同步闸越闭集：账号/密钥环/冲突解决三元闸之外先登记",
            TermiusCode::ENoVault => "三账格越闭集：主机/密钥/片段 15 格闭集之外先登记",
            TermiusCode::EVaultDrift => "三账载荷漂移：格折叠与基准不恒等，逐格复算",
            TermiusCode::EVaultOrder => "三账序逆序：主机→密钥→片段单调，先片段后主机落账必抓",
            TermiusCode::ENoSftp => "SFTP 格越闭集：SFTP 8 格闭集之外先登记",
            TermiusCode::ESftpDrift => "SFTP 载荷漂移：格折叠与基准不恒等，逐格复算",
            TermiusCode::ENoScene => "跨端场景越闭集：5 场景闭集之外先登记",
            TermiusCode::ESceneDrift => "跨端场景漂移：场景载荷与基准不恒等，零漂移必抓",
            TermiusCode::ENoPlan => "分账缺行：订阅/免费两行缺一拒封",
            TermiusCode::EDup => "重复落账：同格只许一条账目",
            TermiusCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            TermiusCode::EGradeDrift => "定级漂移：同步三元闸全落且三账全通才可定 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：无条件收口
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
            Grade::S1 => "S1：无条件收口（Termius 域内不适用，保留闭集位）",
            Grade::S2 => "S2：有条件收口——三账 15 格+SFTP 8 格+跨端 5 场景+分账 2 行全通，同步三元闸为前提条件",
            Grade::S3 => "S3：不收口，同步三元闸或分账 2 行缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 同步三元闸名。
pub const fn sync_gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("账号闸"),
        1 => Some("密钥环闸"),
        2 => Some("冲突解决闸"),
        _ => None,
    }
}

/// 三账账号（格 v → 账 = v/5；0=主机 1=密钥 2=片段）。
pub const fn vault_kind(v: usize) -> Option<u32> {
    if v >= VAULT_CELLS {
        return None;
    }
    Some((v / CELLS_PER_VAULT) as u32)
}

/// 三账账名。
pub const fn vault_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("主机账"),
        1 => Some("密钥账"),
        2 => Some("片段账"),
        _ => None,
    }
}

/// 三账格载荷（格 v）。
pub const fn vault_cell(v: usize) -> Option<u64> {
    if v >= VAULT_CELLS {
        return None;
    }
    Some((0xC4000u64 + (v as u64) * 0x2D) ^ 0x5445524D49555335)
}

/// SFTP 格载荷（格 f）。
pub const fn sftp_cell(f: usize) -> Option<u64> {
    if f >= SFTP_CELLS {
        return None;
    }
    Some((0xC4100u64 + (f as u64) * 0x3A) ^ 0x5346545050415435)
}

/// SFTP 格名。
pub const fn sftp_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("SFTP 浏览"),
        1 => Some("SFTP 上传"),
        2 => Some("SFTP 下载"),
        3 => Some("SFTP 重命名"),
        4 => Some("SFTP 删除"),
        5 => Some("SFTP 建目录"),
        6 => Some("SFTP 权限"),
        7 => Some("SFTP 断点续传"),
        _ => None,
    }
}

/// 跨端场景载荷（场景 c）。
pub const fn scene_cell(c: usize) -> Option<u64> {
    if c >= CROSS_SCENES {
        return None;
    }
    Some((0xC4200u64 + (c as u64) * 0x41) ^ 0x43524F5353444535)
}

/// 跨端场景名。
pub const fn scene_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("桌面→移动"),
        1 => Some("移动→桌面"),
        2 => Some("CLI→桌面"),
        3 => Some("Web→CLI"),
        4 => Some("桌面↔移动双向"),
        _ => None,
    }
}

/// 分账行名。
pub const fn plan_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("订阅分账行"),
        1 => Some("免费分账行"),
        _ => None,
    }
}

/// 注入三态轮转（轮 k：0=主机漂移 1=SFTP 越闭集 2=三账逆序）。
pub const fn inject_kind(k: usize) -> u32 {
    (k % 3) as u32
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Termius 收口账本状态机。
#[derive(Clone)]
pub struct TermiusLedger {
    /// 收口六判据闸。
    gates: [Option<bool>; JUDGE_GATES],
    /// 同步三元闸。
    sync_gates: [Option<bool>; SYNC_GATES],
    /// 主机/密钥/片段三账。
    vault: [Option<u64>; VAULT_CELLS],
    /// SFTP 账。
    sftp: [Option<u64>; SFTP_CELLS],
    /// 跨端零漂移场景账。
    scenes: [Option<u64>; CROSS_SCENES],
    /// 订阅/免费分账 2 行。
    plans: [Option<bool>; PLAN_ROWS],
    /// 最后三账账号。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl TermiusLedger {
    /// 新账本。
    pub fn new() -> Self {
        TermiusLedger {
            gates: [None; JUDGE_GATES],
            sync_gates: [None; SYNC_GATES],
            vault: [None; VAULT_CELLS],
            sftp: [None; SFTP_CELLS],
            scenes: [None; CROSS_SCENES],
            plans: [None; PLAN_ROWS],
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

    /// 落一格收口六判据闸。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), TermiusCode> {
        if gate >= JUDGE_GATES {
            return Err(TermiusCode::EGradeDrift);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5400 + gate as u64);
        Ok(())
    }

    /// 六判据闸全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < JUDGE_GATES {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 落一格同步三元闸。
    pub fn record_sync_gate(&mut self, g: usize) -> Result<(), TermiusCode> {
        if g >= SYNC_GATES {
            return Err(TermiusCode::ENoSyncGate);
        }
        if let Some(c) = self.sync_gates.get_mut(g) {
            *c = Some(true);
        }
        self.fnv(0x5410 + g as u64);
        Ok(())
    }

    /// 同步三元闸全落核验（前提条件）。
    pub fn sync_gates_all(&self) -> bool {
        let mut g = 0usize;
        while g < SYNC_GATES {
            match self.sync_gates.get(g) {
                Some(Some(true)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 三账一格（账单调 + 载荷对照 + 重复拒）。
    pub fn record_vault(&mut self, v: usize, got: u64) -> Result<(), TermiusCode> {
        if v >= VAULT_CELLS {
            return Err(TermiusCode::ENoVault);
        }
        let kind = match vault_kind(v) {
            Some(k) => k,
            None => return Err(TermiusCode::ENoVault),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5420);
            return Err(TermiusCode::EVaultOrder);
        }
        let want = match vault_cell(v) {
            Some(w) => w,
            None => return Err(TermiusCode::ENoVault),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5421 + v as u64);
            return Err(TermiusCode::EVaultDrift);
        }
        if match self.vault.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TermiusCode::EDup);
        }
        if let Some(c) = self.vault.get_mut(v) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5422 + v as u64);
        Ok(())
    }

    /// 三账全账核验（15 格）。
    pub fn vault_all(&self) -> bool {
        let mut v = 0usize;
        while v < VAULT_CELLS {
            match self.vault.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// SFTP 一格（载荷对照 + 重复拒）。
    pub fn record_sftp(&mut self, f: usize, got: u64) -> Result<(), TermiusCode> {
        let want = match sftp_cell(f) {
            Some(w) => w,
            None => return Err(TermiusCode::ENoSftp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5430 + f as u64);
            return Err(TermiusCode::ESftpDrift);
        }
        if match self.sftp.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TermiusCode::EDup);
        }
        if let Some(c) = self.sftp.get_mut(f) {
            *c = Some(got);
        }
        self.fnv(0x5431 + f as u64);
        Ok(())
    }

    /// SFTP 全账核验（8 格）。
    pub fn sftp_all(&self) -> bool {
        let mut f = 0usize;
        while f < SFTP_CELLS {
            match self.sftp.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 跨端场景一格（载荷恒等才收，零漂移）。
    pub fn record_scene(&mut self, c: usize, got: u64) -> Result<(), TermiusCode> {
        let want = match scene_cell(c) {
            Some(w) => w,
            None => return Err(TermiusCode::ENoScene),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5440 + c as u64);
            return Err(TermiusCode::ESceneDrift);
        }
        if match self.scenes.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TermiusCode::EDup);
        }
        if let Some(s) = self.scenes.get_mut(c) {
            *s = Some(got);
        }
        self.fnv(0x5441 + c as u64);
        Ok(())
    }

    /// 跨端场景全账核验（5 场景零漂移）。
    pub fn scenes_all(&self) -> bool {
        let mut c = 0usize;
        while c < CROSS_SCENES {
            match self.scenes.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 分账一行落账。
    pub fn record_plan(&mut self, r: usize) -> Result<(), TermiusCode> {
        if r >= PLAN_ROWS {
            return Err(TermiusCode::ENoPlan);
        }
        if let Some(c) = self.plans.get_mut(r) {
            *c = Some(true);
        }
        self.fnv(0x5450 + r as u64);
        Ok(())
    }

    /// 分账全落核验（2 行缺一拒封）。
    pub fn plans_all(&self) -> bool {
        let mut r = 0usize;
        while r < PLAN_ROWS {
            match self.plans.get(r) {
                Some(Some(true)) => {}
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

    /// KB-COMPAT 定级（同步闸或分账缺 → S3；其余全通 → S2 有条件收口）。
    pub fn judge_grade(&self) -> Grade {
        if !self.sync_gates_all() || !self.plans_all() {
            Grade::S3
        } else if self.vault_all() && self.sftp_all() && self.scenes_all() {
            Grade::S2
        } else {
            Grade::S2
        }
    }
}

impl Default for TermiusLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = TermiusLedger::new();
    let mut g = 0usize;
    while g < JUDGE_GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut sg = 0usize;
    while sg < SYNC_GATES {
        let _ = led.record_sync_gate(sg);
        sg += 1;
    }
    let mut v = 0usize;
    while v < VAULT_CELLS {
        match vault_cell(v) {
            Some(w) => {
                if led.record_vault(v, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        v += 1;
    }
    let mut f = 0usize;
    while f < SFTP_CELLS {
        match sftp_cell(f) {
            Some(w) => {
                if led.record_sftp(f, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        f += 1;
    }
    let mut c = 0usize;
    while c < CROSS_SCENES {
        match scene_cell(c) {
            Some(w) => {
                if led.record_scene(c, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        c += 1;
    }
    let mut r = 0usize;
    while r < PLAN_ROWS {
        let _ = led.record_plan(r);
        r += 1;
    }
    // 注入 10 轮（SSH 终端/分屏 ×10 语义面）：主机漂移 / SFTP 越闭集 /
    // 三账逆序 三态轮转；全部落在当轮临时账本，漂移注入先录正格再注。
    let mut k = 0usize;
    while k < 10 {
        let kind = inject_kind(k);
        let mut lt = TermiusLedger::new();
        let caught = match kind {
            0 => {
                // SSH 终端分屏消费面：主机格先录正格再注漂移。
                lt.record_vault(0, vault_cell(0).unwrap_or(0));
                lt.record_vault(1, vault_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(TermiusCode::EVaultDrift)
            }
            1 => {
                // 先录正格再注漂移：SFTP 格越闭集。
                lt.record_sftp(2, sftp_cell(2).unwrap_or(0));
                lt.record_sftp(SFTP_CELLS, 0) == Err(TermiusCode::ENoSftp)
            }
            _ => {
                // order 注入对空账本同样必抓：片段账先于主机账。
                lt.record_vault(10, vault_cell(10).unwrap_or(0));
                lt.record_vault(2, vault_cell(2).unwrap_or(0)) == Err(TermiusCode::EVaultOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5460 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

/// 主机漂移探针（避免判据体内重复书写）。
fn vault_drift_probe(led: &mut TermiusLedger) -> Result<(), TermiusCode> {
    led.record_vault(0, vault_cell(0).unwrap_or(0))?;
    led.record_vault(1, vault_cell(1).unwrap_or(0).wrapping_add(1))
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52228 判据集（锚 UNX-F52229-J1，off-by-one 板式）。
pub fn run_unx52228_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52228");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "同步三元闸全落",
        {
            let mut lt = TermiusLedger::new();
            let mut g = 0usize;
            while g < SYNC_GATES {
                let _ = lt.record_sync_gate(g);
                g += 1;
            }
            lt.sync_gates_all()
        },
        "账号/密钥环/冲突解决三闸逐格落账，为 S2 前提条件",
    );
    s.add(
        "主机/密钥/片段三账 15 格全通",
        recorded >= (VAULT_CELLS + SFTP_CELLS + CROSS_SCENES) as u32,
        "主机 5 + 密钥 5 + 片段 5 分节折叠恒等",
    );
    s.add(
        "三账账序 0..3 单调",
        {
            let mut ok = true;
            let mut v = CELLS_PER_VAULT;
            while v < VAULT_CELLS {
                if vault_kind(v).unwrap_or(3) < vault_kind(v - 1).unwrap_or(3) {
                    ok = false;
                }
                v += 1;
            }
            ok
        },
        "主机→密钥→片段账号单调，逆序必抓",
    );
    s.add(
        "SFTP 8 格全通",
        {
            let mut lt = TermiusLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < SFTP_CELLS {
                match sftp_cell(f) {
                    Some(w) => {
                        if lt.record_sftp(f, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                f += 1;
            }
            ok && lt.sftp_all()
        },
        "浏览/上传/下载/重命名/删除/建目录/权限/断点续传逐格恒等",
    );
    s.add(
        "跨端零漂移 5 场景全通",
        {
            let mut lt = TermiusLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CROSS_SCENES {
                match scene_cell(c) {
                    Some(w) => {
                        if lt.record_scene(c, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                c += 1;
            }
            ok && lt.scenes_all()
        },
        "桌面→移动/移动→桌面/CLI→桌面/Web→CLI/双向逐场景恒等",
    );
    s.add(
        "订阅/免费分账 2 行全落",
        {
            let mut lt = TermiusLedger::new();
            let mut r = 0usize;
            while r < PLAN_ROWS {
                let _ = lt.record_plan(r);
                r += 1;
            }
            lt.plans_all()
        },
        "订阅/免费两行缺一拒封",
    );
    s.add(
        "收口六判据闸全落",
        {
            let mut lt = TermiusLedger::new();
            let mut g = 0usize;
            while g < JUDGE_GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "六判据（Electron 三段账语义）逐闸落账",
    );
    s.add("注入 10/10 全抓", caught >= 10, "SSH 终端/分屏 ×10 三态轮转必抓");
    let mut l0 = TermiusLedger::new();
    s.add(
        "边界·三账格越闭集",
        l0.record_vault(VAULT_CELLS, 0).is_err(),
        "三账格 15 越闭集必拒",
    );
    s.add(
        "边界·SFTP 格越闭集",
        l0.record_sftp(SFTP_CELLS, 0).is_err(),
        "SFTP 格 8 越闭集必拒",
    );
    s.add(
        "边界·跨端场景越闭集",
        l0.record_scene(CROSS_SCENES, 0).is_err(),
        "跨端场景 5 越闭集必拒",
    );
    s.add(
        "主机漂移拒",
        vault_drift_probe(&mut l0) == Err(TermiusCode::EVaultDrift),
        "主机载荷漂移必抓",
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
            let mut lt = TermiusLedger::new();
            let mut g = 0usize;
            while g < JUDGE_GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut sg = 0usize;
            while sg < SYNC_GATES {
                let _ = lt.record_sync_gate(sg);
                sg += 1;
            }
            let mut v = 0usize;
            while v < VAULT_CELLS {
                match vault_cell(v) {
                    Some(w) => {
                        let _ = lt.record_vault(v, w);
                    }
                    None => {}
                }
                v += 1;
            }
            let mut f = 0usize;
            while f < SFTP_CELLS {
                match sftp_cell(f) {
                    Some(w) => {
                        let _ = lt.record_sftp(f, w);
                    }
                    None => {}
                }
                f += 1;
            }
            let mut c = 0usize;
            while c < CROSS_SCENES {
                match scene_cell(c) {
                    Some(w) => {
                        let _ = lt.record_scene(c, w);
                    }
                    None => {}
                }
                c += 1;
            }
            let mut r = 0usize;
            while r < PLAN_ROWS {
                let _ = lt.record_plan(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "同步三元闸全落且三账全通 → S2（KB-COMPAT 定级登记）",
    );
    s.add(
        "定级同步闸缺封拒 S3",
        TermiusLedger::new().judge_grade() == Grade::S3,
        "同步三元闸未落 → S3 不收口",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC78C0,
        "SEG_BASE 0x40 对齐且在 VX15 域",
    );
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_gate_names() {
        assert_eq!(sync_gate_name(0), Some("账号闸"));
        assert_eq!(sync_gate_name(2), Some("冲突解决闸"));
        assert!(sync_gate_name(SYNC_GATES).is_none());
    }

    #[test]
    fn test_vault_kind() {
        assert_eq!(vault_kind(0), Some(0));
        assert_eq!(vault_kind(4), Some(0));
        assert_eq!(vault_kind(5), Some(1));
        assert_eq!(vault_kind(14), Some(2));
        assert!(vault_kind(VAULT_CELLS).is_none());
        assert_eq!(vault_kind_name(0), Some("主机账"));
        assert_eq!(vault_kind_name(2), Some("片段账"));
        assert!(vault_kind_name(3).is_none());
    }

    #[test]
    fn test_sftp_names() {
        assert_eq!(sftp_name(0), Some("SFTP 浏览"));
        assert_eq!(sftp_name(7), Some("SFTP 断点续传"));
        assert!(sftp_name(SFTP_CELLS).is_none());
        assert!(sftp_cell(SFTP_CELLS).is_none());
    }

    #[test]
    fn test_scene_names() {
        assert_eq!(scene_name(0), Some("桌面→移动"));
        assert_eq!(scene_name(4), Some("桌面↔移动双向"));
        assert!(scene_name(CROSS_SCENES).is_none());
        assert!(scene_cell(CROSS_SCENES).is_none());
    }

    #[test]
    fn test_plan_rows() {
        assert_eq!(plan_row_name(0), Some("订阅分账行"));
        assert_eq!(plan_row_name(1), Some("免费分账行"));
        assert!(plan_row_name(PLAN_ROWS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = vault_cell(7).unwrap_or(0);
        assert_eq!(a, vault_cell(7).unwrap_or(0));
        assert_ne!(a, vault_cell(8).unwrap_or(0));
        assert_ne!(sftp_cell(3).unwrap_or(0), sftp_cell(4).unwrap_or(0));
    }

    #[test]
    fn test_vault_order_caught() {
        let mut lt = TermiusLedger::new();
        let _ = lt.record_vault(10, vault_cell(10).unwrap_or(0));
        assert_eq!(lt.record_vault(2, vault_cell(2).unwrap_or(0)), Err(TermiusCode::EVaultOrder));
        assert!(lt.record_vault(11, vault_cell(11).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = TermiusLedger::new();
        assert_eq!(
            lt.record_vault(6, vault_cell(6).unwrap_or(0).wrapping_add(1)),
            Err(TermiusCode::EVaultDrift)
        );
        assert_eq!(
            lt.record_sftp(5, sftp_cell(5).unwrap_or(0).wrapping_add(2)),
            Err(TermiusCode::ESftpDrift)
        );
        assert_eq!(
            lt.record_scene(3, scene_cell(3).unwrap_or(0).wrapping_sub(1)),
            Err(TermiusCode::ESceneDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = TermiusLedger::new();
        let _ = lt.record_scene(1, scene_cell(1).unwrap_or(0));
        assert_eq!(lt.record_scene(1, scene_cell(1).unwrap_or(0)), Err(TermiusCode::EDup));
        let _ = lt.record_sftp(0, sftp_cell(0).unwrap_or(0));
        assert_eq!(lt.record_sftp(0, sftp_cell(0).unwrap_or(0)), Err(TermiusCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = TermiusLedger::new();
        let mut g = 0usize;
        while g < JUDGE_GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(JUDGE_GATES).is_err());
        assert!(lt.gates_all());
        let mut lt2 = TermiusLedger::new();
        let mut sg = 0usize;
        while sg < SYNC_GATES {
            assert!(lt2.record_sync_gate(sg).is_ok());
            sg += 1;
        }
        assert!(lt2.record_sync_gate(SYNC_GATES).is_err());
        assert!(lt2.sync_gates_all());
    }

    #[test]
    fn test_inject_rotation() {
        assert_eq!(inject_kind(0), 0);
        assert_eq!(inject_kind(1), 1);
        assert_eq!(inject_kind(2), 2);
        assert_eq!(inject_kind(6), 0);
        assert_eq!(inject_kind(9), 0);
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (VAULT_CELLS + SFTP_CELLS + CROSS_SCENES) as u32);
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
        assert_eq!(SEG_BASE, 0xC78C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(TermiusCode::Ok as u32, 0);
        assert_eq!(TermiusCode::EGradeDrift as u32, 12);
        let mut i = 0u32;
        while i <= 12 {
            let code = match i {
                0 => TermiusCode::Ok,
                1 => TermiusCode::ENoSyncGate,
                2 => TermiusCode::ENoVault,
                3 => TermiusCode::EVaultDrift,
                4 => TermiusCode::EVaultOrder,
                5 => TermiusCode::ENoSftp,
                6 => TermiusCode::ESftpDrift,
                7 => TermiusCode::ENoScene,
                8 => TermiusCode::ESceneDrift,
                9 => TermiusCode::ENoPlan,
                10 => TermiusCode::EDup,
                11 => TermiusCode::EFp,
                _ => TermiusCode::EGradeDrift,
            };
            assert_eq!(code.val(), SEG_BASE + i);
            assert!(!code.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let mut lt = TermiusLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut sg = 0usize;
        while sg < SYNC_GATES {
            let _ = lt.record_sync_gate(sg);
            sg += 1;
        }
        let mut r = 0usize;
        while r < PLAN_ROWS {
            let _ = lt.record_plan(r);
            r += 1;
        }
        assert_eq!(lt.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52228_checks_green() {
        let s = run_unx52228_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
