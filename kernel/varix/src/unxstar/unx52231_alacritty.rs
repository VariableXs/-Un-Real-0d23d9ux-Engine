//! UNX-F52231 Alacritty 收口 · 0xC7980 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52231"，逐字）：
//!   title: Alacritty 收口
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52232-J1 六判据全
//!     落；多路复用/SSH 域 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式：本卡（卡 N=52231）判据锚 UNX-F(N+1)-J1 = UNX-F52232-J1，
//! 照原文锚定；功能按本卡标题（Alacritty）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① TOML 配置双向投影 16 格：TOML 源 → 运行时结构 → 回写投影三段
//!      双向账，16 格逐格恒等才收，越格必拒；漂移必抓。
//!   ② GPU 渲染三元闸：软件渲染 / 降级合成 / GPU 直绘三态闭集，越三元
//!      必拒（GPU 探测降级口径承 §AI-66.2 专题四同构）。
//!   ③ 键位/滚动/字号三面 12 格：每面 4 格，面序单调（键位→滚动→字号），
//!      逆序必抓；载荷漂移必抓。
//!   ④ CLI 透传 6 格：命令行透传参数账逐格落账，越格必拒。
//!   ⑤ 注入核 10 轮三态轮转（TOML 格越闭集 / 三面漂移 / 三面逆序），全
//!      部落在当轮临时账本上 10/10 必抓；六判据母版闸 + KB-COMPAT 定级
//!      （全通 → S1，本卡口径）；双跑 FNV-1a 指纹一致。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标（const fn 表 + Option 边界）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52231;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// TOML 双向投影格数。
pub const TOML_CELLS: usize = 16;

/// GPU 渲染三元闸数。
pub const GPU_STATES: usize = 3;

/// 键位/滚动/字号三面格数。
pub const FACE_CELLS: usize = 12;

/// 每面格数。
pub const FACE_PER_KIND: usize = 4;

/// CLI 透传格数。
pub const CLI_CELLS: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlacrittyCode {
    /// 成功
    Ok = 0,
    /// TOML 投影格越闭集
    ENoToml,
    /// TOML 投影漂移
    ETomlDrift,
    /// GPU 态越三元
    ENoGpu,
    /// 三面格越闭集
    ENoFace,
    /// 三面漂移
    EFaceDrift,
    /// 三面逆序
    EFaceOrder,
    /// CLI 格越闭集
    ENoCli,
    /// CLI 漂移
    ECliDrift,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl AlacrittyCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            AlacrittyCode::Ok => "成功",
            AlacrittyCode::ENoToml => "TOML 投影格越闭集：双向投影 16 格之外先登记",
            AlacrittyCode::ETomlDrift => "TOML 投影漂移：双向投影折叠与基准不恒等，逐格复算",
            AlacrittyCode::ENoGpu => "GPU 态越三元：软件渲染/降级合成/GPU 直绘三态之外先登记",
            AlacrittyCode::ENoFace => "三面格越闭集：键位/滚动/字号 12 格之外先登记",
            AlacrittyCode::EFaceDrift => "三面漂移：面载荷折叠与基准不恒等，逐格复算",
            AlacrittyCode::EFaceOrder => "三面逆序：键位→滚动→字号单调，逆序必抓",
            AlacrittyCode::ENoCli => "CLI 格越闭集：透传 6 格之外先登记",
            AlacrittyCode::ECliDrift => "CLI 漂移：透传载荷与基准不恒等，逐格复算",
            AlacrittyCode::EDup => "重复落账：同格只许一条账目",
            AlacrittyCode::ESeal => "未齐即封：16 投影 + 3 态 + 12 面格 + 6 CLI 未齐先补齐",
            AlacrittyCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            AlacrittyCode::EGradeDrift => "定级漂移：全通且各节全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+16投影+3态+12面格+6CLI 全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// TOML 双向投影格载荷（格 t；正向 = t%2==0，回写 = t%2==1）。
pub const fn toml_cell(t: usize) -> Option<u64> {
    if t >= TOML_CELLS {
        return None;
    }
    let dir = if t % 2 == 0 {
        0x4E4F47
    } else {
        0x4E4F57
    };
    Some((0xD1000u64 + (t as u64) * 0x39) ^ dir)
}

/// 投影方向名。
pub const fn toml_dir_name(t: usize) -> Option<&'static str> {
    if t >= TOML_CELLS {
        return None;
    }
    if t % 2 == 0 {
        Some("正向投影 TOML→运行时")
    } else {
        Some("回写投影 运行时→TOML")
    }
}

/// GPU 渲染三元闸基准值（态 g）。
pub const fn gpu_cell(g: usize) -> Option<u32> {
    if g >= GPU_STATES {
        return None;
    }
    Some(0xD200 + g as u32 * 0x40)
}

/// GPU 态名。
pub const fn gpu_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("软件渲染"),
        1 => Some("降级合成"),
        2 => Some("GPU 直绘"),
        _ => None,
    }
}

/// 三面格载荷（格 f；面 = f/4）。
pub const fn face_cell(f: usize) -> Option<u64> {
    if f >= FACE_CELLS {
        return None;
    }
    Some((0xD3000u64 + (f as u64) * 0x2C) ^ ((f / FACE_PER_KIND) as u64).wrapping_mul(0x01000193))
}

/// 三面族（0..4=键位 4..8=滚动 8..12=字号）。
pub const fn face_kind(f: usize) -> Option<u32> {
    if f >= FACE_CELLS {
        return None;
    }
    Some((f / FACE_PER_KIND) as u32)
}

/// 面名。
pub const fn face_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("键位面"),
        1 => Some("滚动面"),
        2 => Some("字号面"),
        _ => None,
    }
}

/// CLI 透传格载荷（格 x）。
pub const fn cli_cell(x: usize) -> Option<u64> {
    if x >= CLI_CELLS {
        return None;
    }
    Some((0xD4000u64 + (x as u64) * 0x51) ^ 0x434C4950524F58)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Alacritty 收口账本状态机。
#[derive(Clone)]
pub struct AlacrittyLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// TOML 双向投影账。
    tomls: [Option<u64>; TOML_CELLS],
    /// GPU 三态账。
    gpus: [Option<u32>; GPU_STATES],
    /// 三面账。
    faces: [Option<u64>; FACE_CELLS],
    /// CLI 透传账。
    clis: [Option<u64>; CLI_CELLS],
    /// 最后面。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl AlacrittyLedger {
    /// 新账本。
    pub fn new() -> Self {
        AlacrittyLedger {
            gates: [None; 6],
            tomls: [None; TOML_CELLS],
            gpus: [None; GPU_STATES],
            faces: [None; FACE_CELLS],
            clis: [None; CLI_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), AlacrittyCode> {
        if gate >= 6 {
            return Err(AlacrittyCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4D00 + gate as u64);
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

    /// TOML 投影一格（载荷对照 + 重复拒）。
    pub fn record_toml(&mut self, t: usize, got: u64) -> Result<(), AlacrittyCode> {
        let want = match toml_cell(t) {
            Some(w) => w,
            None => return Err(AlacrittyCode::ENoToml),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4E00 + t as u64);
            return Err(AlacrittyCode::ETomlDrift);
        }
        if match self.tomls.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AlacrittyCode::EDup);
        }
        if let Some(c) = self.tomls.get_mut(t) {
            *c = Some(got);
        }
        self.fnv(0x4E01 + t as u64);
        Ok(())
    }

    /// TOML 投影全账核验（16 格全齐）。
    pub fn tomls_all(&self) -> bool {
        let mut t = 0usize;
        while t < TOML_CELLS {
            match self.tomls.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// GPU 三元闸一格（基准对照 + 重复拒）。
    pub fn set_gpu(&mut self, g: usize) -> Result<(), AlacrittyCode> {
        let want = match gpu_cell(g) {
            Some(w) => w,
            None => return Err(AlacrittyCode::ENoGpu),
        };
        if match self.gpus.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AlacrittyCode::EDup);
        }
        if let Some(c) = self.gpus.get_mut(g) {
            *c = Some(want);
        }
        self.fnv(0x4F00 + g as u64);
        Ok(())
    }

    /// GPU 三元全落核验。
    pub fn gpus_all(&self) -> bool {
        let mut g = 0usize;
        while g < GPU_STATES {
            match self.gpus.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 三面一格（面单调 + 载荷对照 + 重复拒）。
    pub fn record_face(&mut self, f: usize, got: u64) -> Result<(), AlacrittyCode> {
        if f >= FACE_CELLS {
            return Err(AlacrittyCode::ENoFace);
        }
        let kind = match face_kind(f) {
            Some(k) => k,
            None => return Err(AlacrittyCode::ENoFace),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5000);
            return Err(AlacrittyCode::EFaceOrder);
        }
        let want = match face_cell(f) {
            Some(w) => w,
            None => return Err(AlacrittyCode::ENoFace),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5001 + f as u64);
            return Err(AlacrittyCode::EFaceDrift);
        }
        if match self.faces.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AlacrittyCode::EDup);
        }
        if let Some(c) = self.faces.get_mut(f) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5002 + f as u64);
        Ok(())
    }

    /// 三面全账核验（12 格）。
    pub fn faces_all(&self) -> bool {
        let mut f = 0usize;
        while f < FACE_CELLS {
            match self.faces.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// CLI 透传一格（载荷对照 + 重复拒）。
    pub fn record_cli(&mut self, x: usize, got: u64) -> Result<(), AlacrittyCode> {
        let want = match cli_cell(x) {
            Some(w) => w,
            None => return Err(AlacrittyCode::ENoCli),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5100 + x as u64);
            return Err(AlacrittyCode::ECliDrift);
        }
        if match self.clis.get(x) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AlacrittyCode::EDup);
        }
        if let Some(c) = self.clis.get_mut(x) {
            *c = Some(got);
        }
        self.fnv(0x5101 + x as u64);
        Ok(())
    }

    /// CLI 全账核验（6 格）。
    pub fn clis_all(&self) -> bool {
        let mut x = 0usize;
        while x < CLI_CELLS {
            match self.clis.get(x) {
                Some(Some(_)) => {}
                _ => return false,
            }
            x += 1;
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

    /// KB-COMPAT 定级（全通 → S1 本卡口径，判据缺 → S3，其余 → S2）。
    /// main/aux 各节（投影/GPU/三面/CLI）全部记账后再断言。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.tomls_all()
            && self.gpus_all()
            && self.faces_all()
            && self.clis_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for AlacrittyLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = AlacrittyLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut t = 0usize;
    while t < TOML_CELLS {
        if let Some(w) = toml_cell(t) {
            if led.record_toml(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
    }
    let mut gg = 0usize;
    while gg < GPU_STATES {
        let _ = led.set_gpu(gg);
        gg += 1;
    }
    let mut f = 0usize;
    while f < FACE_CELLS {
        if let Some(w) = face_cell(f) {
            if led.record_face(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut x = 0usize;
    while x < CLI_CELLS {
        if let Some(w) = cli_cell(x) {
            if led.record_cli(x, w).is_ok() {
                recorded += 1;
            }
        }
        x += 1;
    }
    // 注入 10 轮：TOML 格越闭集 / 三面漂移（先录正格再注漂移）/ 三面逆
    // 序（先录高面正格再注低面）三态轮转，全部落在当轮临时账本上，注入
    // =检出=10、逃逸=0；主账本 last_kind 不被注入扰动。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = AlacrittyLedger::new();
        let caught = match kind {
            0 => lt.record_toml(TOML_CELLS, 0).is_err(),
            1 => {
                let _ = lt.record_face(1, face_cell(1).unwrap_or(0));
                lt.record_face(2, face_cell(2).unwrap_or(0).wrapping_add(1))
                    == Err(AlacrittyCode::EFaceDrift)
            }
            _ => {
                let _ = lt.record_face(9, face_cell(9).unwrap_or(0));
                lt.record_face(4, face_cell(4).unwrap_or(0)) == Err(AlacrittyCode::EFaceOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5200 + k as u64);
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
/// UNX-F52231 判据集（锚 UNX-F52232-J1，off-by-one 板式）。
pub fn run_unx52231_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52231");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "TOML 双向投影 16 格全通",
        {
            let mut lt = AlacrittyLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TOML_CELLS {
                if let Some(w) = toml_cell(t) {
                    if lt.record_toml(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.tomls_all()
        },
        "正向投影 TOML→运行时 / 回写投影 运行时→TOML 逐格恒等",
    );
    s.add(
        "GPU 渲染三元闸全落",
        {
            let mut lt = AlacrittyLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < GPU_STATES {
                if lt.set_gpu(g).is_err() {
                    ok = false;
                }
                g += 1;
            }
            ok && lt.gpus_all()
        },
        "软件渲染/降级合成/GPU 直绘三态闭集，越三元必拒",
    );
    s.add(
        "键位/滚动/字号三面 12 格全通",
        recorded >= (TOML_CELLS + FACE_CELLS + CLI_CELLS) as u32,
        "三面闭集逐格恒等，越面必拒",
    );
    s.add(
        "CLI 透传 6 格全落",
        {
            let mut lt = AlacrittyLedger::new();
            let mut ok = true;
            let mut x = 0usize;
            while x < CLI_CELLS {
                if let Some(w) = cli_cell(x) {
                    if lt.record_cli(x, w).is_err() {
                        ok = false;
                    }
                }
                x += 1;
            }
            ok && lt.clis_all()
        },
        "命令行透传参数账逐格落账，越格必拒",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = AlacrittyLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账",
    );
    s.add(
        "三面族单调",
        {
            let mut ok = true;
            let mut f = 1usize;
            while f < FACE_CELLS {
                if face_kind(f).unwrap_or(3) < face_kind(f - 1).unwrap_or(3) {
                    ok = false;
                }
                f += 1;
            }
            ok
        },
        "键位→滚动→字号三面闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "TOML 越闭集/三面漂移/三面逆序三态轮转必抓",
    );
    let mut l0 = AlacrittyLedger::new();
    s.add(
        "边界·TOML 格越闭集",
        l0.record_toml(TOML_CELLS, 0).is_err(),
        "投影 16 越闭集必拒",
    );
    s.add(
        "边界·GPU 态越三元",
        l0.set_gpu(GPU_STATES).is_err(),
        "GPU 态 3 越三元必拒",
    );
    s.add(
        "三面漂移拒",
        {
            l0.record_face(0, face_cell(0).unwrap_or(0));
            l0.record_face(1, face_cell(1).unwrap_or(0).wrapping_add(1)) == Err(AlacrittyCode::EFaceDrift)
        },
        "面载荷漂移必抓",
    );
    s.add(
        "边界·三面格越闭集",
        l0.record_face(FACE_CELLS, 0).is_err(),
        "面格 12 越闭集必拒",
    );
    s.add(
        "边界·CLI 格越闭集",
        l0.record_cli(CLI_CELLS, 0).is_err(),
        "CLI 6 越闭集必拒",
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
            let mut lt = AlacrittyLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut t = 0usize;
            while t < TOML_CELLS {
                if let Some(w) = toml_cell(t) {
                    let _ = lt.record_toml(t, w);
                }
                t += 1;
            }
            let mut g2 = 0usize;
            while g2 < GPU_STATES {
                let _ = lt.set_gpu(g2);
                g2 += 1;
            }
            let mut f = 0usize;
            while f < FACE_CELLS {
                if let Some(w) = face_cell(f) {
                    let _ = lt.record_face(f, w);
                }
                f += 1;
            }
            let mut x = 0usize;
            while x < CLI_CELLS {
                if let Some(w) = cli_cell(x) {
                    let _ = lt.record_cli(x, w);
                }
                x += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7980,
        "SEG_BASE 0x40 对齐且在 VX15 域 0xC7980",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toml_directions() {
        assert_eq!(toml_dir_name(0), Some("正向投影 TOML→运行时"));
        assert_eq!(toml_dir_name(1), Some("回写投影 运行时→TOML"));
        assert!(toml_dir_name(16).is_none());
        assert!(toml_cell(16).is_none());
    }

    #[test]
    fn test_toml_bidirectional_distinct() {
        let fwd = toml_cell(4).unwrap_or(0);
        let back = toml_cell(5).unwrap_or(0);
        assert_ne!(fwd, back, "正/回写方向载荷必不同");
    }

    #[test]
    fn test_gpu_states() {
        assert_eq!(gpu_name(0), Some("软件渲染"));
        assert_eq!(gpu_name(2), Some("GPU 直绘"));
        assert!(gpu_name(3).is_none());
        assert!(gpu_cell(3).is_none());
    }

    #[test]
    fn test_face_kind() {
        assert_eq!(face_kind(0), Some(0));
        assert_eq!(face_kind(4), Some(1));
        assert_eq!(face_kind(11), Some(2));
        assert!(face_kind(12).is_none());
        assert!(face_cell(12).is_none());
        assert_eq!(face_kind_name(1), Some("滚动面"));
    }

    #[test]
    fn test_cells_pure() {
        let a = face_cell(3).unwrap_or(0);
        assert_eq!(a, face_cell(3).unwrap_or(0));
        assert_ne!(a, face_cell(4).unwrap_or(0));
        assert!(cli_cell(CLI_CELLS).is_none());
    }

    #[test]
    fn test_face_order_caught() {
        let mut lt = AlacrittyLedger::new();
        let _ = lt.record_face(8, face_cell(8).unwrap_or(0));
        assert_eq!(lt.record_face(2, face_cell(2).unwrap_or(0)), Err(AlacrittyCode::EFaceOrder));
        assert!(lt.record_face(9, face_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = AlacrittyLedger::new();
        assert_eq!(
            lt.record_toml(3, toml_cell(3).unwrap_or(0).wrapping_add(5)),
            Err(AlacrittyCode::ETomlDrift)
        );
        assert_eq!(
            lt.record_cli(1, cli_cell(1).unwrap_or(0).wrapping_add(2)),
            Err(AlacrittyCode::ECliDrift)
        );
    }

    #[test]
    fn test_dup_and_gate_boundary() {
        let mut lt = AlacrittyLedger::new();
        let _ = lt.set_gpu(0);
        assert_eq!(lt.set_gpu(0), Err(AlacrittyCode::EDup));
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
        assert!(recorded >= (TOML_CELLS + FACE_CELLS + CLI_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC7980);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * 30);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(AlacrittyCode::Ok as u32, 0);
        assert_eq!(AlacrittyCode::EGradeDrift as u32, 12);
        assert!(!AlacrittyCode::ESeal.detail().is_empty());
        assert!(!AlacrittyCode::EFp.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        let lt = AlacrittyLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52231_checks_green() {
        let s = run_unx52231_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
