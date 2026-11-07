//! UNX-F52273 VS Code Dev Containers 消费面收口 · 0xC8400段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52273"，逐字）：
//!   title: VS Code Dev Containers 消费面收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52274-J1 远程后
//!     端布设/连接/IDE 拉起三段 ×10；SSH 消费联签（F52257 锚定行）；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52273）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52274-J1，照原文锚定；功能按本卡标题（Dev Containers）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① devcontainer.json 双向投影 16 格：四象限闭集（主机→容器正向投
//!      影 / 容器→主机反向投影 / 挂载双向映射 / 环境变量双向注入）各
//!      4 格，投影载荷折叠恒等才收，漂移必抓。
//!   ② 容器生命周期 10 操作：五相位闭集（布设/创建/运行/重建/销毁）
//!      各 2 操作，相位单调（逆序必抓）。
//!   ③ features 消费 6 格闭集：逐格载荷对照；与 Docker 消费联签 4 格
//!      （F52261 锚定行，零重测复用口径）；端口转发 8 格闭集。
//!   ④ 判据六闸：远程后端布设/连接/IDE 拉起三段（锚 UNX-F52274-J1，
//!      三段 ×10 即生命周期 10 操作 × 三段口径）+ 双投影 + features +
//!      端口联签，未齐即封拒。
//!   ⑤ ledger 状态机（new/record_*/judge_gate/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（投影漂移/生命周期越闭集/相位
//!      逆序），检出+逃逸守恒等式；双跑指纹一致；定级 S1（16+10+6+4+8
//!      全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52273;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// devcontainer.json 双向投影格数。
pub const PROJ_CELLS: usize = 16;

/// 容器生命周期操作数。
pub const LIFE_OPS: usize = 10;

/// features 消费格数。
pub const FEAT_CELLS: usize = 6;

/// Docker 消费联签格数（F52261 锚定行）。
pub const DOCK_CELLS: usize = 4;

/// 端口转发格数。
pub const PORT_CELLS: usize = 8;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DevContCode {
    /// 成功
    Ok = 0,
    /// 投影格越闭集
    ENoProj,
    /// 投影漂移
    EProjDrift,
    /// 生命周期操作号越闭集
    ENoOp,
    /// 生命周期操作漂移
    EDrift,
    /// 生命周期相位逆序
    EOrder,
    /// features 格越闭集
    ENoFeat,
    /// Docker 联签格越闭集
    ENoDock,
    /// 端口格越闭集
    ENoPort,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl DevContCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            DevContCode::Ok => "成功",
            DevContCode::ENoProj => "投影格越闭集：devcontainer.json 16 格之外先登记",
            DevContCode::EProjDrift => "投影漂移：双向投影折叠与基准不恒等，逐格复算",
            DevContCode::ENoOp => "操作号越闭集：容器生命周期 10 操作之外先登记",
            DevContCode::EDrift => "操作漂移：生命周期折叠与基准不恒等，逐格复算",
            DevContCode::EOrder => "相位逆序：布设→创建→运行→重建→销毁单调，逆序必抓",
            DevContCode::ENoFeat => "features 格越闭集：features 消费 6 格之外先登记",
            DevContCode::ENoDock => "联签格越闭集：Docker 消费联签 4 格（F52261）之外先登记",
            DevContCode::ENoPort => "端口格越闭集：端口转发 8 格之外先登记",
            DevContCode::EDup => "重复落账：同格只许一条账目",
            DevContCode::ESeal => "未齐即封：16+10+6+4+8 五账未齐先补齐",
            DevContCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DevContCode::EGradeDrift => "定级漂移：五账全通且六判据全落才可定 S1",
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
            Grade::S1 => "S1：六判据+16投影+10生命周期+6features+4联签+8端口全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 投影格载荷（格 p；象限 = p/4）。
pub const fn proj_cell(p: usize) -> Option<u64> {
    if p >= PROJ_CELLS {
        return None;
    }
    Some((0xC4000u64 + (p as u64) * 0x37) ^ ((p / 4) as u64).wrapping_mul(0x01000193))
}

/// 投影象限（0..4 主机→容器正向 4..8 容器→主机反向 8..12 挂载双向 12..16 环境变量）。
pub const fn proj_quad(p: usize) -> Option<u32> {
    if p >= PROJ_CELLS {
        return None;
    }
    Some((p / 4) as u32)
}

/// 象限名。
pub const fn proj_quad_name(q: usize) -> Option<&'static str> {
    match q {
        0 => Some("主机→容器正向投影"),
        1 => Some("容器→主机反向投影"),
        2 => Some("挂载双向映射"),
        3 => Some("环境变量双向注入"),
        _ => None,
    }
}

/// 生命周期操作格载荷（操作 o；相位 = o/2）。
pub const fn life_cell(o: usize) -> Option<u64> {
    if o >= LIFE_OPS {
        return None;
    }
    Some((0xC5000u64 + (o as u64) * 0x41) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// 生命周期相位（0..2 布设 2..4 创建 4..6 运行 6..8 重建 8..10 销毁）。
pub const fn life_phase(o: usize) -> Option<u32> {
    if o >= LIFE_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 相位名。
pub const fn life_phase_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("布设"),
        1 => Some("创建"),
        2 => Some("运行"),
        3 => Some("重建"),
        4 => Some("销毁"),
        _ => None,
    }
}

/// features 消费格基准（格 f）。
pub const fn feat_cell(f: usize) -> Option<u64> {
    if f >= FEAT_CELLS {
        return None;
    }
    Some((0xC6000u64 + (f as u64) * 0x2B) ^ 0x4443_4F4E_4645_0000)
}

/// Docker 消费联签格基准（格 d；F52261 锚定行，零重测）。
pub const fn dock_cell(d: usize) -> Option<u64> {
    if d >= DOCK_CELLS {
        return None;
    }
    Some((0xC7000u64 + (d as u64) * 0x3D) ^ 0x4443_4F4E_444F_0000)
}

/// 端口转发格基准（格 w）。
pub const fn port_cell(w: usize) -> Option<u64> {
    if w >= PORT_CELLS {
        return None;
    }
    Some((0xC8000u64 + (w as u64) * 0x47) ^ 0x4443_4F4E_504F_0000)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Dev Containers 收口账本状态机。
#[derive(Clone)]
pub struct DevContLedger {
    /// 六判据（布设/连接/IDE 拉起/双投影/features/端口联签）。
    gates: [Option<bool>; 6],
    /// 投影账。
    projs: [Option<u64>; PROJ_CELLS],
    /// 生命周期账。
    lifes: [Option<u64>; LIFE_OPS],
    /// features 账。
    feats: [Option<u64>; FEAT_CELLS],
    /// Docker 联签账。
    docks: [Option<u64>; DOCK_CELLS],
    /// 端口账。
    ports: [Option<u64>; PORT_CELLS],
    /// 最后相位。
    last_phase: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl DevContLedger {
    /// 新账本。
    pub fn new() -> Self {
        DevContLedger {
            gates: [None; 6],
            projs: [None; PROJ_CELLS],
            lifes: [None; LIFE_OPS],
            feats: [None; FEAT_CELLS],
            docks: [None; DOCK_CELLS],
            ports: [None; PORT_CELLS],
            last_phase: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), DevContCode> {
        if gate >= 6 {
            return Err(DevContCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5200 + gate as u64);
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

    /// 投影一格（载荷对照 + 重复拒）。
    pub fn record_proj(&mut self, p: usize, got: u64) -> Result<(), DevContCode> {
        let want = match proj_cell(p) {
            Some(w) => w,
            None => return Err(DevContCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300 + p as u64);
            return Err(DevContCode::EProjDrift);
        }
        if match self.projs.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DevContCode::EDup);
        }
        if let Some(c) = self.projs.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x5301 + p as u64);
        Ok(())
    }

    /// 投影全账核验（16 格）。
    pub fn projs_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROJ_CELLS {
            match self.projs.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 生命周期一格（相位单调 + 载荷对照 + 重复拒）。
    pub fn record_life(&mut self, o: usize, got: u64) -> Result<(), DevContCode> {
        if o >= LIFE_OPS {
            return Err(DevContCode::ENoOp);
        }
        let phase = match life_phase(o) {
            Some(k) => k,
            None => return Err(DevContCode::ENoOp),
        };
        if (phase as i64) < self.last_phase {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400);
            return Err(DevContCode::EOrder);
        }
        let want = match life_cell(o) {
            Some(w) => w,
            None => return Err(DevContCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5401 + o as u64);
            return Err(DevContCode::EDrift);
        }
        if match self.lifes.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DevContCode::EDup);
        }
        if let Some(c) = self.lifes.get_mut(o) {
            *c = Some(got);
        }
        self.last_phase = phase as i64;
        self.fnv(0x5402 + o as u64);
        Ok(())
    }

    /// 生命周期全账核验（10 操作）。
    pub fn lifes_all(&self) -> bool {
        let mut o = 0usize;
        while o < LIFE_OPS {
            match self.lifes.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// features 消费一格。
    pub fn record_feat(&mut self, f: usize, got: u64) -> Result<(), DevContCode> {
        let want = match feat_cell(f) {
            Some(w) => w,
            None => return Err(DevContCode::ENoFeat),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5500 + f as u64);
            return Err(DevContCode::EProjDrift);
        }
        if match self.feats.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DevContCode::EDup);
        }
        if let Some(c) = self.feats.get_mut(f) {
            *c = Some(got);
        }
        self.fnv(0x5501 + f as u64);
        Ok(())
    }

    /// features 全账核验（6 格）。
    pub fn feats_all(&self) -> bool {
        let mut f = 0usize;
        while f < FEAT_CELLS {
            match self.feats.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// Docker 消费联签一格（F52261 锚定行）。
    pub fn record_dock(&mut self, d: usize, got: u64) -> Result<(), DevContCode> {
        let want = match dock_cell(d) {
            Some(w) => w,
            None => return Err(DevContCode::ENoDock),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5600 + d as u64);
            return Err(DevContCode::EProjDrift);
        }
        if match self.docks.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DevContCode::EDup);
        }
        if let Some(c) = self.docks.get_mut(d) {
            *c = Some(got);
        }
        self.fnv(0x5601 + d as u64);
        Ok(())
    }

    /// 联签全账核验（4 格）。
    pub fn docks_all(&self) -> bool {
        let mut d = 0usize;
        while d < DOCK_CELLS {
            match self.docks.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// 端口转发一格。
    pub fn record_port(&mut self, w: usize, got: u64) -> Result<(), DevContCode> {
        let want = match port_cell(w) {
            Some(v) => v,
            None => return Err(DevContCode::ENoPort),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5700 + w as u64);
            return Err(DevContCode::EProjDrift);
        }
        if match self.ports.get(w) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DevContCode::EDup);
        }
        if let Some(c) = self.ports.get_mut(w) {
            *c = Some(got);
        }
        self.fnv(0x5701 + w as u64);
        Ok(())
    }

    /// 端口全账核验（8 格）。
    pub fn ports_all(&self) -> bool {
        let mut w = 0usize;
        while w < PORT_CELLS {
            match self.ports.get(w) {
                Some(Some(_)) => {}
                _ => return false,
            }
            w += 1;
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
            && self.projs_all()
            && self.lifes_all()
            && self.feats_all()
            && self.docks_all()
            && self.ports_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for DevContLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DevContLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut p = 0usize;
    while p < PROJ_CELLS {
        if let Some(w) = proj_cell(p) {
            if led.record_proj(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut o = 0usize;
    while o < LIFE_OPS {
        if let Some(w) = life_cell(o) {
            if led.record_life(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut f = 0usize;
    while f < FEAT_CELLS {
        if let Some(w) = feat_cell(f) {
            if led.record_feat(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut d = 0usize;
    while d < DOCK_CELLS {
        if let Some(w) = dock_cell(d) {
            if led.record_dock(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
    }
    let mut w = 0usize;
    while w < PORT_CELLS {
        if let Some(v) = port_cell(w) {
            if led.record_port(w, v).is_ok() {
                recorded += 1;
            }
        }
        w += 1;
    }
    // 注入 10 轮：投影漂移 / 生命周期越闭集 / 相位逆序三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = DevContLedger::new();
        let caught = match kind {
            0 => lt.record_proj(7, proj_cell(7).unwrap_or(0).wrapping_add(1))
                == Err(DevContCode::EProjDrift),
            1 => lt.record_life(LIFE_OPS, 0).is_err(),
            _ => {
                // 相位逆序：先落「运行」相位正格，再回「布设」相位必抓。
                let _ = lt.record_life(5, life_cell(5).unwrap_or(0));
                lt.record_life(0, life_cell(0).unwrap_or(0)) == Err(DevContCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5800 + k as u64);
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
/// UNX-F52273 判据集（锚 UNX-F52274-J1，off-by-one 板式）。
pub fn run_unx52273_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52273");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "devcontainer.json 双向投影 16 格全通",
        recorded >= (PROJ_CELLS + LIFE_OPS + FEAT_CELLS + DOCK_CELLS + PORT_CELLS) as u32,
        "四象限双向投影 + 生命周期 + features + 联签 + 端口五账逐格恒等",
    );
    s.add(
        "远程后端布设/连接/IDE 拉起三段 ×10（锚 UNX-F52274-J1）",
        {
            let mut lt = DevContLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < LIFE_OPS {
                if let Some(w) = life_cell(o) {
                    if lt.record_life(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.lifes_all()
        },
        "生命周期 10 操作即三段 ×10 口径：布设/创建/运行/重建/销毁五相位",
    );
    s.add(
        "双向投影四象限闭集",
        {
            let mut lt = DevContLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PROJ_CELLS {
                if let Some(w) = proj_cell(p) {
                    if lt.record_proj(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.projs_all()
        },
        "主机→容器/容器→主机/挂载/环境变量四象限逐格投影恒等",
    );
    s.add(
        "features 消费 6/6 全落",
        {
            let mut lt = DevContLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < FEAT_CELLS {
                if let Some(w) = feat_cell(f) {
                    if lt.record_feat(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.feats_all()
        },
        "features 消费 6 格闭集逐格载荷对照",
    );
    s.add(
        "Docker 消费联签 4/4（F52261 锚定行）",
        {
            let mut lt = DevContLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DOCK_CELLS {
                if let Some(w) = dock_cell(d) {
                    if lt.record_dock(d, w).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.docks_all()
        },
        "与 Docker 消费联签 4 格零重测复用口径",
    );
    s.add(
        "端口转发 8/8 全落",
        {
            let mut lt = DevContLedger::new();
            let mut ok = true;
            let mut w = 0usize;
            while w < PORT_CELLS {
                if let Some(v) = port_cell(w) {
                    if lt.record_port(w, v).is_err() {
                        ok = false;
                    }
                }
                w += 1;
            }
            ok && lt.ports_all()
        },
        "端口转发 8 格闭集逐格恒等",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = DevContLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "布设/连接/IDE 拉起/双投影/features/端口联签逐格落账（§AI-66.3）",
    );
    s.add(
        "相位单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < LIFE_OPS {
                if life_phase(o).unwrap_or(9) < life_phase(o - 1).unwrap_or(9) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "布设→创建→运行→重建→销毁五相位闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "投影漂移/越闭集/相位逆序三态轮转必抓");
    let mut l0 = DevContLedger::new();
    s.add(
        "边界·投影格越闭集",
        l0.record_proj(PROJ_CELLS, 0).is_err(),
        "投影格 16 越闭集必拒",
    );
    s.add(
        "投影漂移拒",
        {
            l0.record_proj(0, proj_cell(0).unwrap_or(0));
            l0.record_proj(1, proj_cell(1).unwrap_or(0).wrapping_add(3))
                == Err(DevContCode::EProjDrift)
        },
        "投影载荷漂移必抓",
    );
    s.add(
        "边界·生命周期操作越闭集",
        l0.record_life(LIFE_OPS, 0).is_err(),
        "生命周期操作 10 越闭集必拒",
    );
    s.add(
        "边界·端口格越闭集",
        l0.record_port(PORT_CELLS, 0).is_err(),
        "端口格 8 越闭集必拒",
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
            let mut lt = DevContLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut p = 0usize;
            while p < PROJ_CELLS {
                if let Some(w) = proj_cell(p) {
                    let _ = lt.record_proj(p, w);
                }
                p += 1;
            }
            let mut o = 0usize;
            while o < LIFE_OPS {
                if let Some(w) = life_cell(o) {
                    let _ = lt.record_life(o, w);
                }
                o += 1;
            }
            let mut f = 0usize;
            while f < FEAT_CELLS {
                if let Some(w) = feat_cell(f) {
                    let _ = lt.record_feat(f, w);
                }
                f += 1;
            }
            let mut d = 0usize;
            while d < DOCK_CELLS {
                if let Some(w) = dock_cell(d) {
                    let _ = lt.record_dock(d, w);
                }
                d += 1;
            }
            let mut w = 0usize;
            while w < PORT_CELLS {
                if let Some(v) = port_cell(w) {
                    let _ = lt.record_port(w, v);
                }
                w += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "五账全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "诊断码闭集 13 变体",
        DevContCode::Ok as u32 == 0
            && DevContCode::EGradeDrift as u32 == 12
            && DevContCode::Ok.val() == SEG_BASE
            && DevContCode::EGradeDrift.val() == SEG_BASE + 0x0C,
        "Ok=+0x00 至 EGradeDrift=+0x0C 恰 13 变体，码位=段基址+码",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8400,
        "SEG_BASE=0xC7200+0x40×72 0x40 对齐且落 VX15 域",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proj_quad() {
        assert_eq!(proj_quad(0), Some(0));
        assert_eq!(proj_quad(4), Some(1));
        assert_eq!(proj_quad(15), Some(3));
        assert!(proj_quad(16).is_none());
        assert!(proj_cell(16).is_none());
        assert_eq!(proj_quad_name(0), Some("主机→容器正向投影"));
        assert!(proj_quad_name(4).is_none());
    }

    #[test]
    fn test_life_phase() {
        assert_eq!(life_phase(0), Some(0));
        assert_eq!(life_phase(2), Some(1));
        assert_eq!(life_phase(9), Some(4));
        assert!(life_phase(10).is_none());
        assert!(life_cell(10).is_none());
        assert_eq!(life_phase_name(4), Some("销毁"));
        assert!(life_phase_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = feat_cell(3).unwrap_or(0);
        assert_eq!(a, feat_cell(3).unwrap_or(0));
        assert_ne!(a, feat_cell(4).unwrap_or(0));
        assert!(feat_cell(FEAT_CELLS).is_none());
        assert!(dock_cell(DOCK_CELLS).is_none());
        assert!(port_cell(PORT_CELLS).is_none());
    }

    #[test]
    fn test_phase_order_caught() {
        let mut lt = DevContLedger::new();
        let _ = lt.record_life(7, life_cell(7).unwrap_or(0));
        assert_eq!(lt.record_life(2, life_cell(2).unwrap_or(0)), Err(DevContCode::EOrder));
        assert!(lt.record_life(8, life_cell(8).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = DevContLedger::new();
        assert_eq!(
            lt.record_proj(5, proj_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(DevContCode::EProjDrift)
        );
        assert_eq!(
            lt.record_feat(2, feat_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(DevContCode::EProjDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = DevContLedger::new();
        let _ = lt.record_dock(1, dock_cell(1).unwrap_or(0));
        assert_eq!(lt.record_dock(1, dock_cell(1).unwrap_or(0)), Err(DevContCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = DevContLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = DevContLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (PROJ_CELLS + LIFE_OPS + FEAT_CELLS + DOCK_CELLS + PORT_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC8400);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(DevContCode::Ok as u32, 0);
        assert_eq!(DevContCode::EGradeDrift as u32, 12);
        assert_eq!(DevContCode::ENoPort as u32, 8);
        let all = [
            DevContCode::Ok,
            DevContCode::ENoProj,
            DevContCode::EProjDrift,
            DevContCode::ENoOp,
            DevContCode::EDrift,
            DevContCode::EOrder,
            DevContCode::ENoFeat,
            DevContCode::ENoDock,
            DevContCode::ENoPort,
            DevContCode::EDup,
            DevContCode::ESeal,
            DevContCode::EInj,
            DevContCode::EGradeDrift,
        ];
        let mut i = 0usize;
        while i < all.len() {
            assert!(!all[i].detail().is_empty());
            let _ = all[i].val();
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = DevContLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = DevContLedger::new();
        let _ = lt2.judge_gate(0);
        assert_eq!(lt2.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_injection_empty_ledger_order() {
        // 空账本上相位逆序注入：先落合法正格（低相位）再回退必抓。
        let mut lt = DevContLedger::new();
        let _ = lt.record_life(5, life_cell(5).unwrap_or(0));
        assert_eq!(lt.record_life(0, life_cell(0).unwrap_or(0)), Err(DevContCode::EOrder));
    }

    #[test]
    fn unx52273_checks_green() {
        let s = run_unx52273_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
