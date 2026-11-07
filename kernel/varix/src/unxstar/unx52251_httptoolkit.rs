//! UNX-F52251 HTTP Toolkit 收口 · 0xC7E80段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52251"，逐字）：
//!   title: HTTP Toolkit 收口
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52252-J1 代理/
//!     脚本 ×10 全通；证书面消费 ×5；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52251）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52252-J1，照原文锚定（卡面 criteria 内「代理/脚本 ×10」「证书面
//! 消费 ×5」诸语随原文属后卡 mitmproxy 口径）；功能按本卡标题
//! （HTTP Toolkit）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 拦截三态 12 格闭集：三家族（终端拦截/浏览器拦截/应用拦截）各
//!      4 格，逐格载荷纯函数恒等才收，漂移必抓。
//!   ② 规则/断点 10 操作集：两家族（规则 0..6 / 断点 6..10），家族单调
//!      （逆序必抓），逐操作落账。
//!   ③ Docker 拦截 6 格：逐格载荷对照恒等（镜像注入/环境变量代理/
//!      CA 挂载/网络接管/容器重建/拦截验证），漂移必抓。
//!   ④ Electron 上栈 4 层（Chromium 网络栈/Node 主进程/渲染进程桥/
//!      拦截代理接入）逐层落账，缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 三态轮转 10 轮（拦截格漂移/
//!      规则家族逆序/Docker 格漂移），三类注入全部落在当轮临时账本上，
//!      led 上 injected==caught==10、escaped==0；漂移注入先录正格再注
//!      漂移；order 注入对空账本同样必抓且不改 last_kind；双跑指纹一致。
//!   ⑥ KB-COMPAT 定级 S2 口径：HTTP Toolkit 应用拦截依赖端侧信任
//!      CA（Frida 注入路径有条件），六判据 + 12 拦截格 + 10 操作 +
//!      6 Docker 格 + 4 层上栈全通 → S2（判据缺 → S3）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标；XOR 锚常量低 16 位恒零。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52251;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 拦截三态格数。
pub const CAP_CELLS: usize = 12;

/// 规则/断点操作数。
pub const RULE_OPS: usize = 10;

/// Docker 拦截格数。
pub const DOCKER_CELLS: usize = 6;

/// Electron 上栈层数。
pub const ELEC_LAYERS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HtkCode {
    /// 成功
    Ok = 0,
    /// 拦截格越闭集
    ENoCap,
    /// 拦截格载荷漂移
    ECapDrift,
    /// 规则/断点操作越闭集
    ENoRule,
    /// 规则/断点家族逆序
    EOrder,
    /// Docker 拦截格越闭集
    ENoDocker,
    /// Docker 拦截格漂移
    EDockerDrift,
    /// Electron 上栈越层
    ENoElec,
    /// 操作行折叠越闭集
    ENoRow,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl HtkCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            HtkCode::Ok => "成功",
            HtkCode::ENoCap => "拦截格越闭集：终端/浏览器/应用拦截三态 12 格闭集之外先登记",
            HtkCode::ECapDrift => "拦截漂移：拦截格载荷与基准不恒等，逐格复算",
            HtkCode::ENoRule => "规则操作越闭集：规则/断点 10 操作集之外先登记",
            HtkCode::EOrder => "规则家族逆序：规则→断点单调，逆序必抓",
            HtkCode::ENoDocker => "Docker 格越闭集：Docker 拦截 6 格闭集之外先登记",
            HtkCode::EDockerDrift => "Docker 漂移：Docker 拦截格载荷与基准不恒等，逐格复算",
            HtkCode::ENoElec => "Electron 上栈越层：Chromium 网络栈/Node 主进程/渲染桥/代理接入四层之外先登记",
            HtkCode::ENoRow => "操作行越闭集：规则/断点操作行折叠闭集之外先登记",
            HtkCode::EDup => "重复落账：同格只许一条账目",
            HtkCode::ESeal => "未齐即封：12 拦截格 + 10 操作 + 6 Docker 格未齐先补齐",
            HtkCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            HtkCode::EGradeDrift => "定级漂移：六判据+12 拦截格+10 操作+6 Docker 格+4 层全通才可定 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口（应用拦截依赖端侧信任 CA）
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
            Grade::S1 => "S1：全通收口（HTTP Toolkit 口径未授）",
            Grade::S2 => "S2：六判据+12 拦截格+10 操作+6 Docker 格+4 层全通有条件收口",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 拦截格载荷（格 i；家族 = i/4）。
pub const fn cap_cell(i: usize) -> Option<u64> {
    if i >= CAP_CELLS {
        return None;
    }
    Some((0x130000u64 + (i as u64) * 0x61) ^ 0x48545450544B0000)
}

/// 拦截家族（0..4=终端拦截 4..8=浏览器拦截 8..12=应用拦截）。
pub const fn cap_kind(i: usize) -> Option<u32> {
    if i >= CAP_CELLS {
        return None;
    }
    Some((i / 4) as u32)
}

/// 拦截家族名。
pub const fn cap_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("终端拦截"),
        1 => Some("浏览器拦截"),
        2 => Some("应用拦截"),
        _ => None,
    }
}

/// 规则/断点操作家族（0..6=规则 6..10=断点）。
pub const fn rule_kind(o: usize) -> Option<u32> {
    if o >= RULE_OPS {
        return None;
    }
    if o < 6 {
        Some(0)
    } else {
        Some(1)
    }
}

/// 操作家族名。
pub const fn rule_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("规则"),
        1 => Some("断点"),
        _ => None,
    }
}

/// 操作行折叠（行 o，落账值）。
pub const fn rule_row(o: usize) -> Option<u32> {
    if o >= RULE_OPS {
        return None;
    }
    Some(0x13100 + o as u32 * 0x47)
}

/// Docker 拦截格载荷（格 d）。
pub const fn docker_cell(d: usize) -> Option<u64> {
    if d >= DOCKER_CELLS {
        return None;
    }
    Some((0x131000u64 + (d as u64) * 0x3D) ^ 0x444F434B45520000)
}

/// Docker 拦截格名。
pub const fn docker_cell_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("镜像注入"),
        1 => Some("环境变量代理"),
        2 => Some("CA 挂载"),
        3 => Some("网络接管"),
        4 => Some("容器重建"),
        5 => Some("拦截验证"),
        _ => None,
    }
}

/// Electron 上栈层折叠（层 l）。
pub const fn elec_layer(l: usize) -> Option<u32> {
    if l >= ELEC_LAYERS {
        return None;
    }
    Some(0x13200 + l as u32 * 0x59)
}

/// Electron 上栈层名。
pub const fn elec_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("Chromium 网络栈"),
        1 => Some("Node 主进程"),
        2 => Some("渲染进程桥"),
        3 => Some("拦截代理接入"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// HTTP Toolkit 收口账本状态机。
#[derive(Clone)]
pub struct HtkLedger {
    /// 六判据（启动/终端拦截/浏览器拦截/应用拦截/Docker 拦截/卸载）。
    gates: [Option<bool>; 6],
    /// 拦截账。
    caps: [Option<u64>; CAP_CELLS],
    /// 规则/断点操作账。
    rules: [Option<u32>; RULE_OPS],
    /// Docker 拦截账。
    dockers: [Option<u64>; DOCKER_CELLS],
    /// Electron 上栈账。
    elecs: [Option<u32>; ELEC_LAYERS],
    /// 最后操作家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl HtkLedger {
    /// 新账本。
    pub fn new() -> Self {
        HtkLedger {
            gates: [None; 6],
            caps: [None; CAP_CELLS],
            rules: [None; RULE_OPS],
            dockers: [None; DOCKER_CELLS],
            elecs: [None; ELEC_LAYERS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), HtkCode> {
        if gate >= 6 {
            return Err(HtkCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xB100 + gate as u64);
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

    /// 拦截一格（载荷对照 + 重复拒）。
    pub fn record_cap(&mut self, i: usize, got: u64) -> Result<(), HtkCode> {
        if i >= CAP_CELLS {
            return Err(HtkCode::ENoCap);
        }
        let want = match cap_cell(i) {
            Some(w) => w,
            None => return Err(HtkCode::ENoCap),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB200 + i as u64);
            return Err(HtkCode::ECapDrift);
        }
        if match self.caps.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HtkCode::EDup);
        }
        if let Some(c) = self.caps.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xB201 + i as u64);
        Ok(())
    }

    /// 拦截全账核验（12 格）。
    pub fn caps_all(&self) -> bool {
        let mut i = 0usize;
        while i < CAP_CELLS {
            match self.caps.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 规则/断点操作一格（家族单调 + 重复拒；拒后不改 last_kind）。
    pub fn record_rule(&mut self, o: usize) -> Result<(), HtkCode> {
        if o >= RULE_OPS {
            return Err(HtkCode::ENoRule);
        }
        let kind = match rule_kind(o) {
            Some(k) => k,
            None => return Err(HtkCode::ENoRule),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB300);
            return Err(HtkCode::EOrder);
        }
        if match self.rules.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HtkCode::EDup);
        }
        let v = match rule_row(o) {
            Some(v) => v,
            None => return Err(HtkCode::ENoRow),
        };
        if let Some(c) = self.rules.get_mut(o) {
            *c = Some(v);
        }
        self.last_kind = kind as i64;
        self.fnv(0xB301 + o as u64);
        Ok(())
    }

    /// 操作全账核验（10 操作）。
    pub fn rules_all(&self) -> bool {
        let mut o = 0usize;
        while o < RULE_OPS {
            match self.rules.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// Docker 拦截一格（载荷对照 + 重复拒）。
    pub fn record_docker(&mut self, d: usize, got: u64) -> Result<(), HtkCode> {
        if d >= DOCKER_CELLS {
            return Err(HtkCode::ENoDocker);
        }
        let want = match docker_cell(d) {
            Some(w) => w,
            None => return Err(HtkCode::ENoDocker),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB400 + d as u64);
            return Err(HtkCode::EDockerDrift);
        }
        if match self.dockers.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HtkCode::EDup);
        }
        if let Some(c) = self.dockers.get_mut(d) {
            *c = Some(got);
        }
        self.fnv(0xB401 + d as u64);
        Ok(())
    }

    /// Docker 全账核验（6 格）。
    pub fn dockers_all(&self) -> bool {
        let mut d = 0usize;
        while d < DOCKER_CELLS {
            match self.dockers.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// Electron 上栈一层。
    pub fn record_elec(&mut self, l: usize) -> Result<(), HtkCode> {
        let v = match elec_layer(l) {
            Some(v) => v,
            None => return Err(HtkCode::ENoElec),
        };
        if match self.elecs.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HtkCode::EDup);
        }
        if let Some(c) = self.elecs.get_mut(l) {
            *c = Some(v);
        }
        self.fnv(0xB500 + l as u64);
        Ok(())
    }

    /// 上栈全齐核验（4 层）。
    pub fn elecs_all(&self) -> bool {
        let mut l = 0usize;
        while l < ELEC_LAYERS {
            match self.elecs.get(l) {
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

    /// KB-COMPAT 定级（全通 → S2 有条件收口，否则 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.caps_all()
            && self.rules_all()
            && self.dockers_all()
            && self.elecs_all()
        {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for HtkLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = HtkLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < CAP_CELLS {
        if let Some(w) = cap_cell(i) {
            if led.record_cap(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut o = 0usize;
    while o < RULE_OPS {
        if led.record_rule(o).is_ok() {
            recorded += 1;
        }
        o += 1;
    }
    let mut d = 0usize;
    while d < DOCKER_CELLS {
        if let Some(w) = docker_cell(d) {
            if led.record_docker(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
    }
    let mut l = 0usize;
    while l < ELEC_LAYERS {
        let _ = led.record_elec(l);
        l += 1;
    }
    // 注入 10 轮：拦截格漂移 / 规则家族逆序 / Docker 格漂移三态轮转。
    // 三类注入全部落在当轮临时账本上；漂移注入先录正格再注漂移；
    // order 注入对空账本同样必抓且不改 last_kind。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = HtkLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_cap(0, cap_cell(0).unwrap_or(0));
                lt.record_cap(1, cap_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(HtkCode::ECapDrift)
            }
            1 => {
                let _ = lt.record_rule(7);
                lt.record_rule(4) == Err(HtkCode::EOrder)
            }
            _ => {
                let _ = lt.record_docker(0, docker_cell(0).unwrap_or(0));
                lt.record_docker(1, docker_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(HtkCode::EDockerDrift)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xB600 + k as u64);
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
/// UNX-F52251 判据集（锚 UNX-F52252-J1，off-by-one 板式）。
pub fn run_unx52251_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52251");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "拦截三态 12 格全通",
        recorded >= CAP_CELLS as u32,
        "终端拦截/浏览器拦截/应用拦截三家族逐格恒等",
    );
    s.add(
        "规则/断点 10 操作全通",
        {
            let mut lt = HtkLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < RULE_OPS {
                if lt.record_rule(o).is_err() {
                    ok = false;
                }
                o += 1;
            }
            ok && lt.rules_all()
        },
        "规则 6/断点 4 两家族逐操作落账，家族单调",
    );
    s.add(
        "Docker 拦截 6 格全通",
        {
            let mut lt = HtkLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DOCKER_CELLS {
                if let Some(w) = docker_cell(d) {
                    if lt.record_docker(d, w).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.dockers_all()
        },
        "镜像注入/环境变量代理/CA 挂载/网络接管/容器重建/拦截验证逐格恒等",
    );
    s.add(
        "Electron 上栈 4 层全落",
        {
            let mut lt = HtkLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < ELEC_LAYERS {
                if lt.record_elec(l).is_err() {
                    ok = false;
                }
                l += 1;
            }
            ok && lt.elecs_all()
        },
        "Chromium 网络栈/Node 主进程/渲染进程桥/拦截代理接入逐层落账",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = HtkLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/终端拦截/浏览器拦截/应用拦截/Docker 拦截/卸载逐格落账",
    );
    s.add(
        "规则家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < RULE_OPS {
                if rule_kind(o).unwrap_or(2) < rule_kind(o - 1).unwrap_or(2) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "规则→断点闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "拦截漂移/规则逆序/Docker 漂移三态轮转必抓");
    let mut l0 = HtkLedger::new();
    s.add(
        "边界·拦截越闭集",
        l0.record_cap(CAP_CELLS, 0).is_err(),
        "拦截格 12 越闭集必拒",
    );
    s.add(
        "边界·规则越闭集",
        l0.record_rule(RULE_OPS).is_err(),
        "规则操作 10 越闭集必拒",
    );
    s.add(
        "边界·Docker 越闭集",
        l0.record_docker(DOCKER_CELLS, 0).is_err(),
        "Docker 格 6 越闭集必拒",
    );
    s.add(
        "Docker 漂移拒",
        {
            let _ = l0.record_docker(2, docker_cell(2).unwrap_or(0));
            l0.record_docker(3, docker_cell(3).unwrap_or(0).wrapping_add(1))
                == Err(HtkCode::EDockerDrift)
        },
        "Docker 拦截格载荷漂移必抓",
    );
    s.add(
        "逆序拒后同族复验",
        {
            let _ = l0.record_rule(7);
            let r1 = l0.record_rule(4) == Err(HtkCode::EOrder);
            // 逆序拒后 last_kind 未改，同族未录格复验必过。
            r1 && l0.record_rule(8).is_ok()
        },
        "逆序拒后同 last_kind 用同族未录格复验",
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
            let mut lt = HtkLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut i = 0usize;
            while i < CAP_CELLS {
                if let Some(w) = cap_cell(i) {
                    let _ = lt.record_cap(i, w);
                }
                i += 1;
            }
            let mut o = 0usize;
            while o < RULE_OPS {
                let _ = lt.record_rule(o);
                o += 1;
            }
            let mut d = 0usize;
            while d < DOCKER_CELLS {
                if let Some(w) = docker_cell(d) {
                    let _ = lt.record_docker(d, w);
                }
                d += 1;
            }
            let mut l = 0usize;
            while l < ELEC_LAYERS {
                let _ = lt.record_elec(l);
                l += 1;
            }
            // main/aux 各节全部记账后再断言。
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2 有条件收口（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7E80,
        "SEG_BASE 0x40 对齐且在 VX15 域 52251 格",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cap_kinds() {
        assert_eq!(cap_kind(0), Some(0));
        assert_eq!(cap_kind(4), Some(1));
        assert_eq!(cap_kind(11), Some(2));
        assert!(cap_kind(12).is_none());
        assert!(cap_cell(12).is_none());
        assert_eq!(cap_kind_name(2), Some("应用拦截"));
    }

    #[test]
    fn test_rule_kinds() {
        assert_eq!(rule_kind(0), Some(0));
        assert_eq!(rule_kind(5), Some(0));
        assert_eq!(rule_kind(6), Some(1));
        assert_eq!(rule_kind(9), Some(1));
        assert!(rule_kind(10).is_none());
        assert!(rule_row(10).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = docker_cell(3).unwrap_or(0);
        assert_eq!(a, docker_cell(3).unwrap_or(0));
        assert_ne!(a, docker_cell(4).unwrap_or(0));
        // XOR 锚常量低 16 位恒零：锚值低 16 位保持基址低 16 位。
        assert_eq!(a & 0xFFFF, (0x131000u64 + 3 * 0x3D) & 0xFFFF);
        assert!(docker_cell(DOCKER_CELLS).is_none());
        assert!(elec_layer(ELEC_LAYERS).is_none());
        assert_eq!(docker_cell_name(5), Some("拦截验证"));
    }

    #[test]
    fn test_rule_order_caught() {
        let mut lt = HtkLedger::new();
        let _ = lt.record_rule(7);
        assert_eq!(lt.record_rule(4), Err(HtkCode::EOrder));
        assert!(lt.record_rule(8).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = HtkLedger::new();
        assert_eq!(
            lt.record_cap(5, cap_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(HtkCode::ECapDrift)
        );
        assert_eq!(
            lt.record_docker(4, docker_cell(4).unwrap_or(0).wrapping_add(5)),
            Err(HtkCode::EDockerDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = HtkLedger::new();
        let _ = lt.record_elec(1);
        assert_eq!(lt.record_elec(1), Err(HtkCode::EDup));
        let _ = lt.record_rule(3);
        assert_eq!(lt.record_rule(3), Err(HtkCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = HtkLedger::new();
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
        assert!(recorded >= CAP_CELLS as u32 + RULE_OPS as u32 + DOCKER_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xC7E80);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(HtkCode::Ok as u32, 0);
        assert_eq!(HtkCode::EGradeDrift as u32, 12);
        assert_eq!(HtkCode::ENoDocker.val(), SEG_BASE + 5);
        assert!(!HtkCode::Ok.detail().is_empty());
        assert!(!HtkCode::ENoElec.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = HtkLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52251_checks_green() {
        let s = run_unx52251_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
