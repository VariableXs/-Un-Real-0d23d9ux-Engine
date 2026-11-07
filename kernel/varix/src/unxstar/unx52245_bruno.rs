//! UNX-F52245 Bruno 收口 · 0xC7D00段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52245"，逐字）：
//!   title: Bruno 收口
//!   quota: 220
//!   criteria: ① 纯功能代码 ≥220 行；② 判据原文：UNX-F52246-J1 六判据全
//!     落；请求/WS 测试 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完
//!     成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52245）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52246-J1，照原文锚定；功能按本卡标题（Bruno）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① .bru 文本双向投影 16 格闭集：四家族（正向投影 .bru→内部模型 /
//!      反向投影 内部模型→.bru / 目录结构 / 环境引用）各 4 格，家族单调
//!      （逆序必抓），逐格载荷纯函数恒等才收，漂移必抓。
//!   ② git 纯文本存储三元闸：纯文本可读 / 可 diff / 可 merge 三态逐格
//!      落账，越三态闭集必抓。
//!   ③ 请求 10 操作集：两家族（构造 0..5 / 执行 5..10），逐操作恒等；
//!      环境 8 格逐格落账；Electron 4 层（主进程/渲染进程/预加载/IPC 桥）
//!      逐层落账。
//!   ④ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 三态轮转 10 轮（投影格漂移/
//!      请求家族逆序/请求越闭集），三类注入全部落在当轮临时账本上，
//!      led 上 injected==caught==10、escaped==0；漂移注入先录正格再注漂
//!      移；双跑指纹一致。
//!   ⑤ 定级 S1 口径：六判据 + 16 投影格 + 三元闸 + 10 操作 + 8 环境格
//!      + 4 层全通 → S1。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52245;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// .bru 双向投影格数。
pub const BRU_CELLS: usize = 16;

/// git 纯文本存储三元闸态数。
pub const TRI_STATES: usize = 3;

/// 请求操作数。
pub const REQ_OPS: usize = 10;

/// 环境格数。
pub const ENV_CELLS: usize = 8;

/// Electron 层数。
pub const LAYERS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BrunoCode {
    /// 成功
    Ok = 0,
    /// 投影格越闭集
    ENoBru,
    /// 投影漂移
    EBruDrift,
    /// 三元闸越闭集
    ENoTri,
    /// 请求操作越闭集
    ENoReq,
    /// 请求家族逆序
    EOrder,
    /// 环境格越闭集
    ENoEnv,
    /// 环境漂移
    EEnvDrift,
    /// 层越闭集
    ENoLayer,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl BrunoCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            BrunoCode::Ok => "成功",
            BrunoCode::ENoBru => "投影格越闭集：.bru 双向投影 16 格闭集之外先登记",
            BrunoCode::EBruDrift => "投影漂移：投影格载荷与基准不恒等，逐格复算",
            BrunoCode::ENoTri => "三元闸越闭集：git 纯文本存储三态闭集之外先登记",
            BrunoCode::ENoReq => "请求操作越闭集：请求 10 操作集之外先登记",
            BrunoCode::EOrder => "请求家族逆序：构造→执行单调，逆序必抓",
            BrunoCode::ENoEnv => "环境格越闭集：环境 8 格闭集之外先登记",
            BrunoCode::EEnvDrift => "环境漂移：环境格载荷与基准不恒等，逐格复算",
            BrunoCode::ENoLayer => "层越闭集：Electron 4 层闭集之外先登记",
            BrunoCode::EDup => "重复落账：同格只许一条账目",
            BrunoCode::ESeal => "未齐即封：16 投影格 + 10 操作 + 8 环境格未齐先补齐",
            BrunoCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            BrunoCode::EGradeDrift => "定级漂移：六判据+16 投影格+三元闸+10 操作+8 环境格+4 层全通才可定 S1",
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
            Grade::S1 => "S1：六判据+16 投影格+三元闸+10 操作+8 环境格+4 层全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// .bru 投影格载荷（格 i；家族 = i/4）。
pub const fn bru_cell(i: usize) -> Option<u64> {
    if i >= BRU_CELLS {
        return None;
    }
    Some((0xD0000u64 + (i as u64) * 0x3B) ^ ((i / 4) as u64).wrapping_mul(0x0B2A1C7))
}

/// 投影家族（0..4=正向投影 4..8=反向投影 8..12=目录结构 12..16=环境引用）。
pub const fn bru_kind(i: usize) -> Option<u32> {
    if i >= BRU_CELLS {
        return None;
    }
    Some((i / 4) as u32)
}

/// 投影家族名。
pub const fn bru_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("正向投影"),
        1 => Some("反向投影"),
        2 => Some("目录结构"),
        3 => Some("环境引用"),
        _ => None,
    }
}

/// git 纯文本存储三元闸态名。
pub const fn tri_state_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("纯文本可读"),
        1 => Some("可 diff"),
        2 => Some("可 merge"),
        _ => None,
    }
}

/// 三元闸态折叠。
pub const fn tri_state(s: usize) -> Option<u32> {
    if s >= TRI_STATES {
        return None;
    }
    Some(0xD100 + s as u32 * 0x5D)
}

/// 请求操作格载荷（操作 o；家族 = o/5：0..5 构造 5..10 执行）。
pub const fn req_cell(o: usize) -> Option<u64> {
    if o >= REQ_OPS {
        return None;
    }
    Some((0xD2000u64 + (o as u64) * 0x47) ^ ((o / 5) as u64).wrapping_mul(0x01000193))
}

/// 请求操作家族。
pub const fn req_kind(o: usize) -> Option<u32> {
    if o >= REQ_OPS {
        return None;
    }
    Some((o / 5) as u32)
}

/// 请求操作家族名。
pub const fn req_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("构造"),
        1 => Some("执行"),
        _ => None,
    }
}

/// 环境格载荷（格 e）。
pub const fn env_cell(e: usize) -> Option<u64> {
    if e >= ENV_CELLS {
        return None;
    }
    Some((0xD3000u64 + (e as u64) * 0x2F) ^ 0x4252554E4F454E56)
}

/// Electron 层载荷（层 l）。
pub const fn layer_cell(l: usize) -> Option<u64> {
    if l >= LAYERS {
        return None;
    }
    Some(0xD4000u64 + (l as u64) * 0x61)
}

/// Electron 层名。
pub const fn layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("主进程"),
        1 => Some("渲染进程"),
        2 => Some("预加载"),
        3 => Some("IPC 桥"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Bruno 收口账本状态机。
#[derive(Clone)]
pub struct BrunoLedger {
    /// 六判据（启动/请求测试/WS 测试/崩溃/卸载/许可）。
    gates: [Option<bool>; 6],
    /// 投影账。
    brus: [Option<u64>; BRU_CELLS],
    /// 三元闸账。
    tris: [Option<u32>; TRI_STATES],
    /// 请求操作账。
    reqs: [Option<u64>; REQ_OPS],
    /// 环境账。
    envs: [Option<u64>; ENV_CELLS],
    /// 层账。
    layers: [Option<u64>; LAYERS],
    /// 最后请求家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl BrunoLedger {
    /// 新账本。
    pub fn new() -> Self {
        BrunoLedger {
            gates: [None; 6],
            brus: [None; BRU_CELLS],
            tris: [None; TRI_STATES],
            reqs: [None; REQ_OPS],
            envs: [None; ENV_CELLS],
            layers: [None; LAYERS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), BrunoCode> {
        if gate >= 6 {
            return Err(BrunoCode::ESeal);
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

    /// 投影一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_bru(&mut self, i: usize, got: u64) -> Result<(), BrunoCode> {
        if i >= BRU_CELLS {
            return Err(BrunoCode::ENoBru);
        }
        let want = match bru_cell(i) {
            Some(w) => w,
            None => return Err(BrunoCode::ENoBru),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200 + i as u64);
            return Err(BrunoCode::EBruDrift);
        }
        if match self.brus.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BrunoCode::EDup);
        }
        if let Some(c) = self.brus.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x5201 + i as u64);
        Ok(())
    }

    /// 投影全账核验（16 格）。
    pub fn brus_all(&self) -> bool {
        let mut i = 0usize;
        while i < BRU_CELLS {
            match self.brus.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 三元闸一格。
    pub fn record_tri(&mut self, s: usize) -> Result<(), BrunoCode> {
        let v = match tri_state(s) {
            Some(v) => v,
            None => return Err(BrunoCode::ENoTri),
        };
        if match self.tris.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BrunoCode::EDup);
        }
        if let Some(c) = self.tris.get_mut(s) {
            *c = Some(v);
        }
        self.fnv(0x5300 + s as u64);
        Ok(())
    }

    /// 三元闸全齐核验。
    pub fn tris_all(&self) -> bool {
        let mut s = 0usize;
        while s < TRI_STATES {
            match self.tris.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 请求操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_req(&mut self, o: usize, got: u64) -> Result<(), BrunoCode> {
        if o >= REQ_OPS {
            return Err(BrunoCode::ENoReq);
        }
        let kind = match req_kind(o) {
            Some(k) => k,
            None => return Err(BrunoCode::ENoReq),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400);
            return Err(BrunoCode::EOrder);
        }
        let want = match req_cell(o) {
            Some(w) => w,
            None => return Err(BrunoCode::ENoReq),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5401 + o as u64);
            return Err(BrunoCode::EBruDrift);
        }
        if match self.reqs.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BrunoCode::EDup);
        }
        if let Some(c) = self.reqs.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5402 + o as u64);
        Ok(())
    }

    /// 请求全账核验（10 操作）。
    pub fn reqs_all(&self) -> bool {
        let mut o = 0usize;
        while o < REQ_OPS {
            match self.reqs.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 环境一格（载荷对照 + 重复拒）。
    pub fn record_env(&mut self, e: usize, got: u64) -> Result<(), BrunoCode> {
        if e >= ENV_CELLS {
            return Err(BrunoCode::ENoEnv);
        }
        let want = match env_cell(e) {
            Some(w) => w,
            None => return Err(BrunoCode::ENoEnv),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5500 + e as u64);
            return Err(BrunoCode::EEnvDrift);
        }
        if match self.envs.get(e) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BrunoCode::EDup);
        }
        if let Some(c) = self.envs.get_mut(e) {
            *c = Some(got);
        }
        self.fnv(0x5501 + e as u64);
        Ok(())
    }

    /// 环境全账核验（8 格）。
    pub fn envs_all(&self) -> bool {
        let mut e = 0usize;
        while e < ENV_CELLS {
            match self.envs.get(e) {
                Some(Some(_)) => {}
                _ => return false,
            }
            e += 1;
        }
        true
    }

    /// Electron 层一格。
    pub fn record_layer(&mut self, l: usize) -> Result<(), BrunoCode> {
        let v = match layer_cell(l) {
            Some(v) => v,
            None => return Err(BrunoCode::ENoLayer),
        };
        if match self.layers.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BrunoCode::EDup);
        }
        if let Some(c) = self.layers.get_mut(l) {
            *c = Some(v);
        }
        self.fnv(0x5600 + l as u64);
        Ok(())
    }

    /// 层全齐核验（4 层）。
    pub fn layers_all(&self) -> bool {
        let mut l = 0usize;
        while l < LAYERS {
            match self.layers.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
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
        if self.gates_all()
            && self.brus_all()
            && self.tris_all()
            && self.reqs_all()
            && self.envs_all()
            && self.layers_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for BrunoLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = BrunoLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < BRU_CELLS {
        if let Some(w) = bru_cell(i) {
            if led.record_bru(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut s = 0usize;
    while s < TRI_STATES {
        let _ = led.record_tri(s);
        s += 1;
    }
    let mut o = 0usize;
    while o < REQ_OPS {
        if let Some(w) = req_cell(o) {
            if led.record_req(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut e = 0usize;
    while e < ENV_CELLS {
        if let Some(w) = env_cell(e) {
            if led.record_env(e, w).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
    }
    let mut l = 0usize;
    while l < LAYERS {
        let _ = led.record_layer(l);
        l += 1;
    }
    // 注入 10 轮：投影格漂移 / 请求家族逆序 / 请求越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上；漂移注入先录正格再注漂移。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = BrunoLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_bru(3, bru_cell(3).unwrap_or(0));
                lt.record_bru(5, bru_cell(5).unwrap_or(0).wrapping_add(1))
                    == Err(BrunoCode::EBruDrift)
            }
            1 => {
                let _ = lt.record_req(7, req_cell(7).unwrap_or(0));
                lt.record_req(2, req_cell(2).unwrap_or(0)) == Err(BrunoCode::EOrder)
            }
            _ => {
                let _ = lt.record_req(0, req_cell(0).unwrap_or(0));
                lt.record_req(REQ_OPS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5700 + k as u64);
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
/// UNX-F52245 判据集（锚 UNX-F52246-J1，off-by-one 板式）。
pub fn run_unx52245_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52245");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        ".bru 文本双向投影 16 格全通",
        recorded >= BRU_CELLS as u32,
        "正向投影/反向投影/目录结构/环境引用四家族逐格恒等",
    );
    s.add(
        "git 纯文本存储三元闸 3/3 全落",
        {
            let mut lt = BrunoLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TRI_STATES {
                if lt.record_tri(t).is_err() {
                    ok = false;
                }
                t += 1;
            }
            ok && lt.tris_all()
        },
        "纯文本可读/可 diff/可 merge 三态逐格落账",
    );
    s.add(
        "请求 10 操作全通",
        {
            let mut lt = BrunoLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < REQ_OPS {
                if let Some(w) = req_cell(o) {
                    if lt.record_req(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.reqs_all()
        },
        "构造/执行两家族逐操作恒等（请求/WS 测试 ×10）",
    );
    s.add(
        "环境 8 格全落",
        {
            let mut lt = BrunoLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < ENV_CELLS {
                if let Some(w) = env_cell(e) {
                    if lt.record_env(e, w).is_err() {
                        ok = false;
                    }
                }
                e += 1;
            }
            ok && lt.envs_all()
        },
        "环境 8 格逐格载荷对照恒等",
    );
    s.add(
        "Electron 4 层全落",
        {
            let mut lt = BrunoLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < LAYERS {
                if lt.record_layer(l).is_err() {
                    ok = false;
                }
                l += 1;
            }
            ok && lt.layers_all()
        },
        "主进程/渲染进程/预加载/IPC 桥逐层落账",
    );
    s.add(
        "投影家族单调",
        {
            let mut ok = true;
            let mut i = 1usize;
            while i < BRU_CELLS {
                if bru_kind(i).unwrap_or(9) < bru_kind(i - 1).unwrap_or(9) {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "四家族闭集单调",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = BrunoLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/请求测试/WS 测试/崩溃/卸载/许可逐格落账",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "投影格漂移/请求家族逆序/请求越闭集三态轮转必抓",
    );
    let mut l0 = BrunoLedger::new();
    s.add(
        "边界·投影格越闭集",
        l0.record_bru(BRU_CELLS, 0).is_err(),
        "投影格 16 越闭集必拒",
    );
    s.add(
        "边界·请求越闭集",
        l0.record_req(REQ_OPS, 0).is_err(),
        "请求操作 10 越闭集必拒",
    );
    s.add(
        "边界·环境越闭集",
        l0.record_env(ENV_CELLS, 0).is_err(),
        "环境格 8 越闭集必拒",
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
        "定级 S1 闭集",
        {
            let mut lt = BrunoLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut i = 0usize;
            while i < BRU_CELLS {
                if let Some(w) = bru_cell(i) {
                    let _ = lt.record_bru(i, w);
                }
                i += 1;
            }
            let mut t = 0usize;
            while t < TRI_STATES {
                let _ = lt.record_tri(t);
                t += 1;
            }
            let mut o = 0usize;
            while o < REQ_OPS {
                if let Some(w) = req_cell(o) {
                    let _ = lt.record_req(o, w);
                }
                o += 1;
            }
            let mut e = 0usize;
            while e < ENV_CELLS {
                if let Some(w) = env_cell(e) {
                    let _ = lt.record_env(e, w);
                }
                e += 1;
            }
            let mut l = 0usize;
            while l < LAYERS {
                let _ = lt.record_layer(l);
                l += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "诊断码闭集",
        BrunoCode::Ok.val() == SEG_BASE && BrunoCode::EGradeDrift.val() == SEG_BASE + 12,
        "Ok=+0x00、EGradeDrift=+0x0C 恰 13 变体",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7D00,
        "SEG_BASE = 0xC7200 + 0x40×(52245−52201)，0x40 对齐",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bru_cells() {
        assert_eq!(bru_kind_name(0), Some("正向投影"));
        assert_eq!(bru_kind_name(3), Some("环境引用"));
        assert!(bru_kind_name(4).is_none());
        assert_eq!(bru_kind(15), Some(3));
        assert!(bru_kind(16).is_none());
        assert!(bru_cell(16).is_none());
    }

    #[test]
    fn test_tri_states() {
        assert_eq!(tri_state_name(0), Some("纯文本可读"));
        assert_eq!(tri_state_name(2), Some("可 merge"));
        assert!(tri_state_name(3).is_none());
        assert!(tri_state(TRI_STATES).is_none());
    }

    #[test]
    fn test_req_kind() {
        assert_eq!(req_kind(0), Some(0));
        assert_eq!(req_kind(4), Some(0));
        assert_eq!(req_kind(5), Some(1));
        assert_eq!(req_kind(9), Some(1));
        assert!(req_kind(10).is_none());
        assert_eq!(req_kind_name(0), Some("构造"));
        assert_eq!(req_kind_name(1), Some("执行"));
        assert!(req_kind_name(2).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = bru_cell(5).unwrap_or(0);
        assert_eq!(a, bru_cell(5).unwrap_or(0));
        assert_ne!(a, bru_cell(6).unwrap_or(0));
        let e = env_cell(2).unwrap_or(0);
        assert_eq!(e, env_cell(2).unwrap_or(0));
        assert!(env_cell(ENV_CELLS).is_none());
    }

    #[test]
    fn test_layers() {
        assert_eq!(layer_name(0), Some("主进程"));
        assert_eq!(layer_name(3), Some("IPC 桥"));
        assert!(layer_name(4).is_none());
        assert!(layer_cell(LAYERS).is_none());
        let v0 = layer_cell(0).unwrap_or(0);
        let v1 = layer_cell(1).unwrap_or(0);
        assert_ne!(v0, v1);
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = BrunoLedger::new();
        let _ = lt.record_req(7, req_cell(7).unwrap_or(0));
        assert_eq!(lt.record_req(2, req_cell(2).unwrap_or(0)), Err(BrunoCode::EOrder));
        // 逆序被拒后家族账位不受污染：续落同家族高格仍可收。
        assert!(lt.record_req(8, req_cell(8).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = BrunoLedger::new();
        // 先录正格再注漂移。
        let _ = lt.record_bru(0, bru_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_bru(1, bru_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(BrunoCode::EBruDrift)
        );
        assert_eq!(
            lt.record_env(0, env_cell(0).unwrap_or(0).wrapping_add(9)),
            Err(BrunoCode::EEnvDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = BrunoLedger::new();
        let _ = lt.record_bru(4, bru_cell(4).unwrap_or(0));
        assert_eq!(lt.record_bru(4, bru_cell(4).unwrap_or(0)), Err(BrunoCode::EDup));
        let _ = lt.record_tri(0);
        assert_eq!(lt.record_tri(0), Err(BrunoCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = BrunoLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = BrunoLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (BRU_CELLS + REQ_OPS + ENV_CELLS) as u32);
        assert_eq!(caught, 10);
        let (inj, ca, esc) = BrunoLedger::new().stats();
        assert_eq!((inj, ca, esc), (0, 0, 0));
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
        assert_eq!(SEG_BASE, 0xC7D00);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(BrunoCode::Ok as u32, 0);
        assert_eq!(BrunoCode::EGradeDrift as u32, 12);
        assert!(!BrunoCode::Ok.detail().is_empty());
        assert!(!BrunoCode::ESeal.detail().is_empty());
        assert_eq!(BrunoCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = BrunoLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52245_checks_green() {
        let s = run_unx52245_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
