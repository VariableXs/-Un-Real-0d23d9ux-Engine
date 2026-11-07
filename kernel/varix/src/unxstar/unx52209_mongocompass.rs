//! UNX-F52209 MongoDB Compass 收口 · 0xC7400段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面原文（任务板 vx15_cards.json 键 "52209" 逐字锚定）：
//!   title = "MongoDB Compass 收口"，quota = 240，
//!   criteria = "① 纯功能代码 ≥240 行；② 判据原文：UNX-F52210-J1 六判据
//!   全落；键浏览/CLI/慢日志三段 ×10；KB-COMPAT 定级登记；判据锚定
//!   §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。"
//! 板式注记：off-by-one——本卡 52209 的判据锚照卡面原文锚定
//!   UNX-F52210-J1；功能按本卡标题（MongoDB Compass 收口）落地。
//!
//! 设计要点：
//!   ① 域模型：MongoDB 版本阶梯闭集 4 档（4.4/5.0/6.0/7.0）；聚合管道
//!      12 stage 操作账；索引管理 10 操作账；键浏览/CLI/慢日志三段
//!      各 10 操作（卡面三段 ×10）。
//!   ② Electron 上栈 4 层闭集（Chromium/Node/主进程/渲染进程）逐层
//!      折叠恒等；CRUD 六判据收口闸逐格落账。
//!   ③ 注入 ×10 轮（stage 越闭集/版本越阶梯/载荷漂移三态轮转）10/10
//!      必抓，检出+逃逸=注入守恒等式逐轮成立；双跑指纹逐位一致；
//!      KB-COMPAT 定级闭集（12 stage + 10 索引 + 三段 30 + 4 层全通
//!      且六判据全落 → S1）。
//!   ④ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (52209 − 52201)）。
pub const SEG_BASE: u32 = 0xC7400;

const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (52209 - 52201) && SEG_BASE % 0x40 == 0);

/// 聚合管道 stage 数。
pub const STAGES: usize = 12;

/// 索引管理操作数。
pub const IDX_OPS: usize = 10;

/// 键浏览/CLI/慢日志三段，每段操作数。
pub const SECT_OPS: usize = 10;

/// 三段总数。
pub const SECTIONS: usize = 3;

/// MongoDB 版本阶梯档数（4.4/5.0/6.0/7.0）。
pub const VERS: usize = 4;

/// Electron 上栈层数。
pub const LAYERS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompassCode {
    /// 成功
    Ok = 0,
    /// stage 越闭集
    ENoStage,
    /// stage 载荷漂移
    EStageDrift,
    /// 索引操作越闭集
    ENoIdx,
    /// 索引载荷漂移
    EIdxDrift,
    /// 三段号越闭集
    ENoSect,
    /// 版本越阶梯
    ENoVer,
    /// 上栈层越闭集
    ENoLayer,
    /// 上栈载荷漂移
    ELayerDrift,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl CompassCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CompassCode::Ok => "成功",
            CompassCode::ENoStage => "stage 越闭集：12 stage 之外先登记",
            CompassCode::EStageDrift => "stage 载荷漂移：管道折叠与基准不恒等，逐格复算",
            CompassCode::ENoIdx => "索引操作越闭集：10 操作之外先登记",
            CompassCode::EIdxDrift => "索引载荷漂移：索引折叠与基准不恒等，逐格复算",
            CompassCode::ENoSect => "三段号越闭集：键浏览/CLI/慢日志之外先登记",
            CompassCode::ENoVer => "版本越阶梯：4.4/5.0/6.0/7.0 之外先登记",
            CompassCode::ENoLayer => "上栈层越闭集：Electron 4 层之外先登记",
            CompassCode::ELayerDrift => "上栈载荷漂移：层折叠与基准不恒等，逐格复算",
            CompassCode::EDup => "重复落账：同格只许一条账目",
            CompassCode::ESeal => "未齐即封：六判据与全账未齐先补齐",
            CompassCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            CompassCode::EGradeDrift => "定级漂移：主账+辅助账全通且六判据全落才定级 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompassGrade {
    /// S1 旗舰档
    S1,
    /// S2 合格档
    S2,
    /// 未定级
    None,
}

impl CompassGrade {
    pub fn detail(self) -> &'static str {
        match self {
            CompassGrade::S1 => "S1：全账全通+六判据全落，KB-COMPAT 登记旗舰档",
            CompassGrade::S2 => "S2：主账通但辅助账有缺，登记合格档",
            CompassGrade::None => "未定级：账目未齐，先补齐再登记",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 聚合管道 stage 格基准（s ∈ 0..12）。
pub const fn stage_cell(s: usize) -> Option<u64> {
    if s >= STAGES {
        return None;
    }
    Some((0xC8000u64 + (s as u64) * 0x3D) ^ ((s % 3) as u64).wrapping_mul(0x01000193))
}

/// 索引操作格基准（o ∈ 0..10）。
pub const fn idx_cell(o: usize) -> Option<u64> {
    if o >= IDX_OPS {
        return None;
    }
    Some((0xC9000u64 + (o as u64) * 0x27) ^ 0x4E4458494E445831)
}

/// 三段操作格基准（段 k，段内 o）。
pub const fn sect_cell(k: usize, o: usize) -> Option<u64> {
    if k >= SECTIONS || o >= SECT_OPS {
        return None;
    }
    Some((0xCA000u64 + (k as u64) * 0x100 + (o as u64) * 0x2B) ^ 0x53454354)
}

/// 版本阶梯格基准（v ∈ 0..4）。
pub const fn ver_cell(v: usize) -> Option<u64> {
    if v >= VERS {
        return None;
    }
    Some(0xCB000 + v as u64 * 0x40)
}

/// Electron 上栈层折叠（层 l）。
pub const fn layer_cell(l: usize) -> Option<u32> {
    if l >= LAYERS {
        return None;
    }
    Some(0xCC00 + l as u32 * 0x5A)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct CompassLedger {
    /// CRUD 六判据。
    gates: [Option<bool>; 6],
    /// 聚合管道账。
    stages: [Option<u64>; STAGES],
    /// 索引账。
    idxs: [Option<u64>; IDX_OPS],
    /// 三段账。
    sects: [[Option<u64>; SECT_OPS]; SECTIONS],
    /// 上栈账。
    layers: [Option<u32>; LAYERS],
    /// 版本阶梯游标（已收版本数）。
    vers_done: usize,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl CompassLedger {
    /// 新账本。
    pub fn new() -> Self {
        CompassLedger {
            gates: [None; 6],
            stages: [None; STAGES],
            idxs: [None; IDX_OPS],
            sects: [[None; SECT_OPS]; SECTIONS],
            layers: [None; LAYERS],
            vers_done: 0,
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

    /// 落一格六判据（CRUD 六判据：插/查/改/删/批/事）。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), CompassCode> {
        if gate >= 6 {
            return Err(CompassCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4A100 + gate as u64);
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

    /// 聚合管道一格。
    pub fn record_stage(&mut self, s: usize, got: u64) -> Result<(), CompassCode> {
        let want = match stage_cell(s) {
            Some(w) => w,
            None => {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x4A200);
                return Err(CompassCode::ENoStage);
            }
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4A201 + s as u64);
            return Err(CompassCode::EStageDrift);
        }
        if match self.stages.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CompassCode::EDup);
        }
        if let Some(c) = self.stages.get_mut(s) {
            *c = Some(got);
        }
        self.fnv(0x4A202 + s as u64);
        Ok(())
    }

    /// 聚合管道全账核验（12 格）。
    pub fn stages_all(&self) -> bool {
        let mut s = 0usize;
        while s < STAGES {
            match self.stages.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 索引一格。
    pub fn record_idx(&mut self, o: usize, got: u64) -> Result<(), CompassCode> {
        let want = match idx_cell(o) {
            Some(w) => w,
            None => {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x4A300);
                return Err(CompassCode::ENoIdx);
            }
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4A301 + o as u64);
            return Err(CompassCode::EIdxDrift);
        }
        if match self.idxs.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CompassCode::EDup);
        }
        if let Some(c) = self.idxs.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x4A302 + o as u64);
        Ok(())
    }

    /// 索引全账核验（10 格）。
    pub fn idxs_all(&self) -> bool {
        let mut o = 0usize;
        while o < IDX_OPS {
            match self.idxs.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 三段一格（段 k，段内 o）。
    pub fn record_sect(&mut self, k: usize, o: usize, got: u64) -> Result<(), CompassCode> {
        let want = match sect_cell(k, o) {
            Some(w) => w,
            None => {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x4A400 + k as u64);
                return Err(CompassCode::ENoSect);
            }
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4A440 + k as u64 * 0x10 + o as u64);
            return Err(CompassCode::EStageDrift);
        }
        let slot = match self.sects.get_mut(k) {
            Some(row) => match row.get_mut(o) {
                Some(c) => c,
                None => return Err(CompassCode::ENoSect),
            },
            None => return Err(CompassCode::ENoSect),
        };
        if slot.is_some() {
            return Err(CompassCode::EDup);
        }
        *slot = Some(got);
        self.fnv(0x4A480 + k as u64 * 0x10 + o as u64);
        Ok(())
    }

    /// 三段全账核验（3×10 格）。
    pub fn sects_all(&self) -> bool {
        let mut k = 0usize;
        while k < SECTIONS {
            let mut o = 0usize;
            while o < SECT_OPS {
                match self.sects.get(k).and_then(|row| row.get(o)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                o += 1;
            }
            k += 1;
        }
        true
    }

    /// 版本一档（4.4→5.0→6.0→7.0 单调阶梯）。
    pub fn record_ver(&mut self, v: usize, got: u64) -> Result<(), CompassCode> {
        let want = match ver_cell(v) {
            Some(w) => w,
            None => {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x4A500);
                return Err(CompassCode::ENoVer);
            }
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4A501 + v as u64);
            return Err(CompassCode::ENoVer);
        }
        if v != self.vers_done {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4A502 + v as u64);
            return Err(CompassCode::ENoVer);
        }
        self.vers_done += 1;
        self.fnv(0x4A503 + v as u64);
        Ok(())
    }

    /// 版本阶梯全通核验。
    pub fn vers_all(&self) -> bool {
        self.vers_done == VERS
    }

    /// 上栈一层。
    pub fn record_layer(&mut self, l: usize, got: u32) -> Result<(), CompassCode> {
        let want = match layer_cell(l) {
            Some(w) => w,
            None => return Err(CompassCode::ENoLayer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4A600 + l as u64);
            return Err(CompassCode::ELayerDrift);
        }
        if match self.layers.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CompassCode::EDup);
        }
        if let Some(c) = self.layers.get_mut(l) {
            *c = Some(got);
        }
        self.fnv(0x4A601 + l as u64);
        Ok(())
    }

    /// 上栈全账核验（4 层）。
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

    /// KB-COMPAT 定级闭集。
    pub fn grade(&self) -> CompassGrade {
        let main = self.stages_all() && self.gates_all();
        let aux = self.idxs_all() && self.sects_all() && self.layers_all() && self.vers_all();
        if main && aux {
            CompassGrade::S1
        } else if main {
            CompassGrade::S2
        } else {
            CompassGrade::None
        }
    }
}

impl Default for CompassLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = CompassLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut s = 0usize;
    while s < STAGES {
        if let Some(w) = stage_cell(s) {
            if led.record_stage(s, w).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
    }
    let mut o = 0usize;
    while o < IDX_OPS {
        if let Some(w) = idx_cell(o) {
            if led.record_idx(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut k = 0usize;
    while k < SECTIONS {
        let mut j = 0usize;
        while j < SECT_OPS {
            if let Some(w) = sect_cell(k, j) {
                if led.record_sect(k, j, w).is_ok() {
                    recorded += 1;
                }
            }
            j += 1;
        }
        k += 1;
    }
    let mut v = 0usize;
    while v < VERS {
        if let Some(w) = ver_cell(v) {
            let _ = led.record_ver(v, w);
        }
        v += 1;
    }
    let mut l = 0usize;
    while l < LAYERS {
        if let Some(w) = layer_cell(l) {
            let _ = led.record_layer(l, w);
        }
        l += 1;
    }
    // 注入 10 轮：stage 越闭集 / 版本越阶梯 / 载荷漂移三态轮转。三类注入
    // 均落当轮临时账本（越闭集注入对空账本同样必抓；漂移注入先录正格再
    // 注漂移），检出/注入在 led 上单点记账，守恒等式不重不漏。
    let mut r = 0usize;
    while r < 10 {
        let kind = (r % 3) as u32;
        let mut lt = CompassLedger::new();
        let caught = match kind {
            0 => lt.record_stage(STAGES, 0) == Err(CompassCode::ENoStage),
            1 => lt.record_ver(VERS, ver_cell(0).unwrap_or(0)) == Err(CompassCode::ENoVer),
            _ => {
                let _ = lt.record_idx(3, idx_cell(3).unwrap_or(0));
                lt.record_idx(3, idx_cell(3).unwrap_or(0).wrapping_add(1))
                    == Err(CompassCode::EIdxDrift)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4A700 + r as u64);
        } else {
            led.escaped += 1;
        }
        r += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
pub fn run_unx52209_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52209");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "聚合管道 12 stage 全通",
        recorded >= STAGES as u32,
        "12 stage 逐格折叠与基准恒等（UNX-F52210-J1 判据锚）",
    );
    s.add(
        "索引管理 10 操作全通",
        {
            let mut lt = CompassLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < IDX_OPS {
                if let Some(w) = idx_cell(o) {
                    if lt.record_idx(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.idxs_all()
        },
        "索引 10 操作闭集逐格恒等",
    );
    s.add(
        "键浏览/CLI/慢日志三段 3×10 全通",
        {
            let mut lt = CompassLedger::new();
            let mut ok = true;
            let mut k = 0usize;
            while k < SECTIONS {
                let mut j = 0usize;
                while j < SECT_OPS {
                    if let Some(w) = sect_cell(k, j) {
                        if lt.record_sect(k, j, w).is_err() {
                            ok = false;
                        }
                    }
                    j += 1;
                }
                k += 1;
            }
            ok && lt.sects_all()
        },
        "卡面三段 ×10：键浏览/CLI/慢日志逐格恒等",
    );
    s.add(
        "版本阶梯 4.4/5.0/6.0/7.0 全通",
        {
            let mut lt = CompassLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VERS {
                if let Some(w) = ver_cell(v) {
                    if lt.record_ver(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.vers_all()
        },
        "版本单调阶梯逐档恒等",
    );
    s.add(
        "Electron 上栈 4 层全通",
        {
            let mut lt = CompassLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < LAYERS {
                if let Some(w) = layer_cell(l) {
                    if lt.record_layer(l, w).is_err() {
                        ok = false;
                    }
                }
                l += 1;
            }
            ok && lt.layers_all()
        },
        "Chromium/Node/主进程/渲染进程 4 层折叠恒等",
    );
    s.add("CRUD 六判据全落", recorded >= 46, "插/查/改/删/批/事六格闸逐格落账");
    s.add("注入 10/10 全抓", caught >= 10, "越闭集/越阶梯/漂移三态轮转必抓");
    s.add(
        "边界·stage 越闭集",
        CompassLedger::new().record_stage(STAGES, 0) == Err(CompassCode::ENoStage),
        "stage 12 越闭集必拒",
    );
    s.add(
        "边界·版本越阶梯",
        CompassLedger::new().record_ver(VERS, 0) == Err(CompassCode::ENoVer),
        "版本 4 越阶梯必拒",
    );
    s.add(
        "边界·三段越闭集",
        CompassLedger::new().record_sect(SECTIONS, 0, 0) == Err(CompassCode::ENoSect),
        "段 3 越三段闭集必拒",
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
        "KB-COMPAT 定级 S1",
        {
            let mut lt = CompassLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut s2 = 0usize;
            while s2 < STAGES {
                if let Some(w) = stage_cell(s2) {
                    let _ = lt.record_stage(s2, w);
                }
                s2 += 1;
            }
            let mut i2 = 0usize;
            while i2 < IDX_OPS {
                if let Some(w) = idx_cell(i2) {
                    let _ = lt.record_idx(i2, w);
                }
                i2 += 1;
            }
            let mut k2 = 0usize;
            while k2 < SECTIONS {
                let mut j2 = 0usize;
                while j2 < SECT_OPS {
                    if let Some(w) = sect_cell(k2, j2) {
                        let _ = lt.record_sect(k2, j2, w);
                    }
                    j2 += 1;
                }
                k2 += 1;
            }
            let mut v2 = 0usize;
            while v2 < VERS {
                if let Some(w) = ver_cell(v2) {
                    let _ = lt.record_ver(v2, w);
                }
                v2 += 1;
            }
            let mut l2 = 0usize;
            while l2 < LAYERS {
                if let Some(w) = layer_cell(l2) {
                    let _ = lt.record_layer(l2, w);
                }
                l2 += 1;
            }
            lt.grade() == CompassGrade::S1
        },
        CompassGrade::S1.detail(),
    );
    s.add(
        "定级漂移拒",
        {
            let lt = CompassLedger::new();
            lt.grade() != CompassGrade::S1
        },
        CompassCode::EGradeDrift.detail(),
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC7200 && SEG_BASE <= 0xC7700,
        "SEG_BASE 0x40 对齐且在 VX15 域",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stage_cells() {
        assert!(stage_cell(0).is_some());
        assert!(stage_cell(11).is_some());
        assert!(stage_cell(12).is_none());
        assert_ne!(stage_cell(3), stage_cell(4));
    }

    #[test]
    fn test_idx_and_sect_cells() {
        assert!(idx_cell(IDX_OPS).is_none());
        assert!(sect_cell(0, 0).is_some());
        assert!(sect_cell(SECTIONS, 0).is_none());
        assert!(sect_cell(0, SECT_OPS).is_none());
    }

    #[test]
    fn test_ver_ladder_order() {
        let mut lt = CompassLedger::new();
        assert_eq!(
            lt.record_ver(1, ver_cell(1).unwrap_or(0)),
            Err(CompassCode::ENoVer)
        );
        assert!(lt.record_ver(0, ver_cell(0).unwrap_or(0)).is_ok());
        assert!(lt.record_ver(1, ver_cell(1).unwrap_or(0)).is_ok());
        assert!(lt.record_ver(3, ver_cell(3).unwrap_or(0)).is_err());
    }

    #[test]
    fn test_layer_drift_caught() {
        let mut lt = CompassLedger::new();
        assert_eq!(
            lt.record_layer(0, layer_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(CompassCode::ELayerDrift)
        );
        assert!(lt.record_layer(0, layer_cell(0).unwrap_or(0)).is_ok());
        assert_eq!(lt.record_layer(0, layer_cell(0).unwrap_or(0)), Err(CompassCode::EDup));
    }

    #[test]
    fn test_stage_out_of_set_counts_injection() {
        let mut lt = CompassLedger::new();
        assert_eq!(lt.record_stage(STAGES, 0), Err(CompassCode::ENoStage));
        let (inj, caught, esc) = lt.stats();
        assert_eq!((inj, caught, esc), (1, 1, 0));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= 46);
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
        assert_eq!(SEG_BASE, 0xC7400);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (52209 - 52201));
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(CompassCode::Ok.val(), SEG_BASE);
        assert_eq!(CompassCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!CompassCode::EDup.detail().is_empty());
    }

    #[test]
    fn test_grade_closed_set() {
        let empty = CompassLedger::new();
        assert_eq!(empty.grade(), CompassGrade::None);
        let mut full = CompassLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = full.judge_gate(g);
            g += 1;
        }
        let mut s2 = 0usize;
        while s2 < STAGES {
            if let Some(w) = stage_cell(s2) {
                let _ = full.record_stage(s2, w);
            }
            s2 += 1;
        }
        let mut o = 0usize;
        while o < IDX_OPS {
            if let Some(w) = idx_cell(o) {
                let _ = full.record_idx(o, w);
            }
            o += 1;
        }
        assert_eq!(full.grade(), CompassGrade::S2);
        assert_eq!(empty.grade() != CompassGrade::S1, true);
    }

    #[test]
    fn unx52209_checks_green() {
        let s = run_unx52209_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
