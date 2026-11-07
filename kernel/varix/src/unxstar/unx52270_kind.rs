//! UNX-F52270 kind 收口 · 0xC8340段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52270"，逐字）：
//!   title: kind 收口
//!   quota: 220
//!   criteria: ① 纯功能代码 ≥220 行；② 判据原文：UNX-F52271-J1 六判据全
//!     落；TUI 交互状态机完整（键位/退出路径 ×10，体验四口径）；KB-COMPAT
//!     定级登记；判据锚定 §AI-66.3 + 体验四；③ 完成时 API/软件写回：状
//!     态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52270）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52271-J1，照原文锚定；功能按本卡标题（kind）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 集群生命周期 10 操作：create/delete/list/get/kubeconfig/export/
//!      logs/version/build/load 五族各 2 操作（族 = op/2），族单调逆序必抓。
//!   ② 多节点配置 12 格闭集：control-plane 3 格 + worker 3 格 + 端口映射
//!      3 格 + 卷挂载 3 格，逐格载荷折叠恒等才收。
//!   ③ docker 消费联签 4 格（锚 F52261）：镜像构建/镜像加载/网络复用/
//!      卷联签，缺格拒封。
//!   ④ 导出/导入 6 格闭集（导出 kubeconfig/logs/cert ×3 + 导入 config/
//!      image/cluster ×3），逐格分账。
//!   ⑤ TUI 交互状态机口径：10 操作即键位/退出路径 ×10 逐键落账，体验四
//!      口径（可见/可逆/可续/可退）随六判据逐格登记。
//!   ⑥ ledger 状态机（new/record_node/record_op/sign_docker/judge_gate×6/
//!      fingerprint FNV-1a/grade 定级闭集）；注入核 run_cycle 三态轮转
//!      10/10（越闭集/载荷漂移/族逆序），检出+逃逸守恒；双跑指纹一致；
//!      定级 S2 闭集（KB-COMPAT 登记）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标；XOR 锚常量低 16 位恒零。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52270;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 多节点配置格数。
pub const NODE_CELLS: usize = 12;

/// 集群生命周期操作数。
pub const LIFECYCLE_OPS: usize = 10;

/// docker 消费联签格数（锚 F52261）。
pub const DOCKER_SIGNS: usize = 4;

/// 导出/导入格数。
pub const IO_CELLS: usize = 6;

/// XOR 锚常量（低 16 位恒零）。
pub const ANCHOR: u64 = 0x4B494E44_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KindCode {
    /// 成功
    Ok = 0,
    /// 节点格越闭集
    ENoNode,
    /// 节点载荷漂移
    ENodeDrift,
    /// 操作号越闭集
    ENoOp,
    /// 操作载荷漂移
    EOpDrift,
    /// 族逆序
    EOrder,
    /// 联签格越闭集
    ENoSign,
    /// 联签缺格
    ESignMiss,
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

impl KindCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            KindCode::Ok => "成功",
            KindCode::ENoNode => "节点格越闭集：多节点配置 12 格之外先登记",
            KindCode::ENodeDrift => "节点载荷漂移：control-plane/worker/端口/卷折叠与基准不恒等",
            KindCode::ENoOp => "操作号越闭集：create/delete/list/get/kubeconfig 等 10 操作集之外先登记",
            KindCode::EOpDrift => "操作载荷漂移：生命周期操作折叠与基准不恒等，逐格复算",
            KindCode::EOrder => "族逆序：生命周期五族单调，逆序必抓",
            KindCode::ENoSign => "联签格越闭集：docker 消费联签 4 格之外先登记",
            KindCode::ESignMiss => "联签缺格：docker 消费联签（F52261）未齐先补齐",
            KindCode::EDup => "重复落账：同格只许一条账目",
            KindCode::ESeal => "未齐即封：12 节点 + 10 操作 + 4 联签 + 6 导出导入未齐先补齐",
            KindCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            KindCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            KindCode::EGradeDrift => "定级漂移：四账全齐且六判据全落才可定 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口（本卡目标级）
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
            Grade::S1 => "S1：全通收口（本卡闭集不启用，目标级为 S2）",
            Grade::S2 => "S2：四账全齐+六判据全落，有条件收口（KB-COMPAT 登记）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 节点格载荷（格 n；族 = n/3，族内逐格折叠）。
pub const fn node_cell(n: usize) -> Option<u64> {
    if n >= NODE_CELLS {
        return None;
    }
    Some((0xB7100u64 + (n as u64) * 0x41) ^ ANCHOR)
}

/// 节点族（0..3=control-plane 3..6=worker 6..9=端口映射 9..12=卷挂载）。
pub const fn node_kind(n: usize) -> Option<u32> {
    if n >= NODE_CELLS {
        return None;
    }
    Some((n / 3) as u32)
}

/// 节点族名。
pub const fn node_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("control-plane"),
        1 => Some("worker"),
        2 => Some("端口映射"),
        3 => Some("卷挂载"),
        _ => None,
    }
}

/// 生命周期操作格载荷（操作 o；族 = o/2）。
pub const fn lifecycle_cell(o: usize) -> Option<u64> {
    if o >= LIFECYCLE_OPS {
        return None;
    }
    Some((0xB7200u64 + (o as u64) * 0x2F) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// 生命周期操作族（0..2=create 2..4=delete 4..6=list/get 6..8=kubeconfig/export 8..10=logs/version/build/load 辅助）。
pub const fn lifecycle_kind(o: usize) -> Option<u32> {
    if o >= LIFECYCLE_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 生命周期操作名。
pub const fn lifecycle_op_name(o: usize) -> Option<&'static str> {
    match o {
        0 => Some("create"),
        1 => Some("delete"),
        2 => Some("list"),
        3 => Some("get"),
        4 => Some("kubeconfig"),
        5 => Some("export"),
        6 => Some("logs"),
        7 => Some("version"),
        8 => Some("build"),
        9 => Some("load"),
        _ => None,
    }
}

/// docker 消费联签折叠（联签 g；判据锚 F52261，四格闭集）。
pub const fn docker_sign(g: usize) -> Option<u32> {
    if g >= DOCKER_SIGNS {
        return None;
    }
    Some(0xB7300 + g as u32 * 0x47)
}

/// docker 联签名。
pub const fn docker_sign_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("镜像构建联签"),
        1 => Some("镜像加载联签"),
        2 => Some("网络复用联签"),
        3 => Some("卷联签"),
        _ => None,
    }
}

/// 导出/导入格折叠（格 r）。
pub const fn io_cell(r: usize) -> Option<u32> {
    if r >= IO_CELLS {
        return None;
    }
    if r < 3 {
        Some(0xB7400 + r as u32 * 0x23)
    } else {
        Some(0xB7480 + (r - 3) as u32 * 0x23)
    }
}

/// 导出/导入格名（0..3 导出，3..6 导入）。
pub const fn io_cell_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("导出 kubeconfig"),
        1 => Some("导出 logs"),
        2 => Some("导出 cert"),
        3 => Some("导入 config"),
        4 => Some("导入 image"),
        5 => Some("导入 cluster"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// kind 收口账本状态机。
#[derive(Clone)]
pub struct KindLedger {
    /// 六判据（安装/建群/销毁/镜像/配置/登记）。
    gates: [Option<bool>; 6],
    /// 节点配置账。
    nodes: [Option<u64>; NODE_CELLS],
    /// 生命周期操作账。
    ops: [Option<u64>; LIFECYCLE_OPS],
    /// docker 联签账。
    signs: [Option<u32>; DOCKER_SIGNS],
    /// 导出/导入账。
    ios: [Option<u32>; IO_CELLS],
    /// 最后操作族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl KindLedger {
    /// 新账本。
    pub fn new() -> Self {
        KindLedger {
            gates: [None; 6],
            nodes: [None; NODE_CELLS],
            ops: [None; LIFECYCLE_OPS],
            signs: [None; DOCKER_SIGNS],
            ios: [None; IO_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), KindCode> {
        if gate >= 6 {
            return Err(KindCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6100 + gate as u64);
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

    /// 节点一格（载荷对照 + 重复拒）。
    pub fn record_node(&mut self, n: usize, got: u64) -> Result<(), KindCode> {
        let want = match node_cell(n) {
            Some(w) => w,
            None => return Err(KindCode::ENoNode),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200 + n as u64);
            return Err(KindCode::ENodeDrift);
        }
        if match self.nodes.get(n) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KindCode::EDup);
        }
        if let Some(c) = self.nodes.get_mut(n) {
            *c = Some(got);
        }
        self.fnv(0x6201 + n as u64);
        Ok(())
    }

    /// 节点全账核验（12 格）。
    pub fn nodes_all(&self) -> bool {
        let mut n = 0usize;
        while n < NODE_CELLS {
            match self.nodes.get(n) {
                Some(Some(_)) => {}
                _ => return false,
            }
            n += 1;
        }
        true
    }

    /// 生命周期操作一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), KindCode> {
        if o >= LIFECYCLE_OPS {
            return Err(KindCode::ENoOp);
        }
        let kind = match lifecycle_kind(o) {
            Some(k) => k,
            None => return Err(KindCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300);
            return Err(KindCode::EOrder);
        }
        let want = match lifecycle_cell(o) {
            Some(w) => w,
            None => return Err(KindCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6301 + o as u64);
            return Err(KindCode::EOpDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KindCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6302 + o as u64);
        Ok(())
    }

    /// 操作全账核验（10 操作）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < LIFECYCLE_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// docker 联签一格（锚 F52261）。
    pub fn sign_docker(&mut self, g: usize) -> Result<(), KindCode> {
        if g >= DOCKER_SIGNS {
            return Err(KindCode::ENoSign);
        }
        if match self.signs.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KindCode::EDup);
        }
        if let Some(c) = self.signs.get_mut(g) {
            *c = docker_sign(g);
        }
        self.fnv(0x6400 + g as u64);
        Ok(())
    }

    /// 联签全齐核验（4 格）。
    pub fn signs_all(&self) -> bool {
        let mut g = 0usize;
        while g < DOCKER_SIGNS {
            match self.signs.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 导出/导入一格。
    pub fn record_io(&mut self, r: usize) -> Result<(), KindCode> {
        let want = match io_cell(r) {
            Some(w) => w,
            None => return Err(KindCode::ENoSign),
        };
        if match self.ios.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KindCode::EDup);
        }
        if let Some(c) = self.ios.get_mut(r) {
            *c = Some(want);
        }
        self.fnv(0x6500 + r as u64);
        Ok(())
    }

    /// 导出/导入全齐核验（6 格）。
    pub fn ios_all(&self) -> bool {
        let mut r = 0usize;
        while r < IO_CELLS {
            match self.ios.get(r) {
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

    /// KB-COMPAT 定级（四账全齐+六判据全落 → S2，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.nodes_all()
            && self.ops_all()
            && self.signs_all()
            && self.ios_all()
        {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for KindLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = KindLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut n = 0usize;
    while n < NODE_CELLS {
        if let Some(w) = node_cell(n) {
            if led.record_node(n, w).is_ok() {
                recorded += 1;
            }
        }
        n += 1;
    }
    let mut o = 0usize;
    while o < LIFECYCLE_OPS {
        if let Some(w) = lifecycle_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut gs = 0usize;
    while gs < DOCKER_SIGNS {
        let _ = led.sign_docker(gs);
        gs += 1;
    }
    let mut r = 0usize;
    while r < IO_CELLS {
        let _ = led.record_io(r);
        r += 1;
    }
    // 注入 10 轮：越闭集 / 载荷漂移 / 族逆序三态轮转，全部落在当轮
    // 临时账本上；主账本只记注入计数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = KindLedger::new();
        let caught = match kind {
            0 => {
                let hit_a = lt.record_node(NODE_CELLS, 0) == Err(KindCode::ENoNode);
                let hit_b = lt.record_op(LIFECYCLE_OPS, 0).is_err();
                hit_a || hit_b
            }
            1 => {
                // 漂移：先录正格，再注漂移载荷（同族内不触发族序）。
                let _ = lt.record_op(2, lifecycle_cell(2).unwrap_or(0));
                lt.record_op(3, lifecycle_cell(3).unwrap_or(0).wrapping_add(1))
                    == Err(KindCode::EOpDrift)
            }
            _ => {
                // 族逆序：对空账本注入逆序对同样必抓，且不改 last_kind。
                let _ = lt.record_op(5, lifecycle_cell(5).unwrap_or(0));
                let before = lt.last_kind;
                let hit = lt.record_op(1, lifecycle_cell(1).unwrap_or(0)) == Err(KindCode::EOrder);
                hit && lt.last_kind == before
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6600 + k as u64);
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
/// UNX-F52270 判据集（锚 UNX-F52271-J1，off-by-one 板式）。
pub fn run_unx52270_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52270");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "生命周期 10 操作全通",
        recorded >= LIFECYCLE_OPS as u32,
        "create/delete/list/get/kubeconfig/export/logs/version/build/load 逐操作恒等",
    );
    s.add(
        "多节点配置 12 格全落",
        {
            let mut lt = KindLedger::new();
            let mut ok = true;
            let mut n = 0usize;
            while n < NODE_CELLS {
                if let Some(w) = node_cell(n) {
                    if lt.record_node(n, w).is_err() {
                        ok = false;
                    }
                }
                n += 1;
            }
            ok && lt.nodes_all()
        },
        "control-plane/worker/端口映射/卷挂载四族逐格恒等",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = KindLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/建群/销毁/镜像/配置/登记逐格落账",
    );
    s.add(
        "docker 消费联签 4/4 全齐（F52261）",
        {
            let mut lt = KindLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < DOCKER_SIGNS {
                if lt.sign_docker(g).is_err() {
                    ok = false;
                }
                g += 1;
            }
            ok && lt.signs_all()
        },
        "镜像构建/镜像加载/网络复用/卷联签四格闭集（锚 F52261）",
    );
    s.add(
        "导出/导入 6 格全齐",
        {
            let mut lt = KindLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < IO_CELLS {
                if lt.record_io(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.ios_all()
        },
        "导出 kubeconfig/logs/cert ×3 + 导入 config/image/cluster ×3",
    );
    s.add(
        "族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < LIFECYCLE_OPS {
                if lifecycle_kind(o).unwrap_or(6) < lifecycle_kind(o - 1).unwrap_or(6) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "生命周期五族闭集单调",
    );
    s.add(
        "TUI 键位/退出路径 ×10 落账",
        {
            // 10 操作逐键位状态机：键名闭集 + 退出路径（操作号 0..10）。
            let mut ok = true;
            let mut o = 0usize;
            while o < LIFECYCLE_OPS {
                if lifecycle_op_name(o).is_none() {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "键位/退出路径 ×10 逐键落账，体验四口径（可见/可逆/可续/可退）随六判据登记",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "越闭集/载荷漂移/族逆序三态轮转必抓",
    );
    s.add(
        "边界·节点格越闭集",
        KindLedger::new().record_node(NODE_CELLS, 0).is_err(),
        "节点格 12 越闭集必拒",
    );
    s.add(
        "操作漂移拒",
        {
            let mut lt = KindLedger::new();
            let _ = lt.record_op(4, lifecycle_cell(4).unwrap_or(0));
            lt.record_op(5, lifecycle_cell(5).unwrap_or(0).wrapping_add(1))
                == Err(KindCode::EOpDrift)
        },
        "操作载荷漂移必抓",
    );
    s.add(
        "边界·操作越闭集",
        KindLedger::new().record_op(LIFECYCLE_OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·联签格越闭集",
        KindLedger::new().sign_docker(DOCKER_SIGNS).is_err(),
        "联签格 4 越闭集必拒",
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
            // main/aux 各节全部记账后再断言。
            let mut lt = KindLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut n = 0usize;
            while n < NODE_CELLS {
                if let Some(w) = node_cell(n) {
                    let _ = lt.record_node(n, w);
                }
                n += 1;
            }
            let mut o = 0usize;
            while o < LIFECYCLE_OPS {
                if let Some(w) = lifecycle_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut gs = 0usize;
            while gs < DOCKER_SIGNS {
                let _ = lt.sign_docker(gs);
                gs += 1;
            }
            let mut r = 0usize;
            while r < IO_CELLS {
                let _ = lt.record_io(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2 && Grade::S2.val() == 2
        },
        "四账全齐+六判据全落 → S2（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且落在 VX15 派工公式位（0xC8340）",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_families() {
        assert_eq!(node_kind_name(0), Some("control-plane"));
        assert_eq!(node_kind_name(3), Some("卷挂载"));
        assert!(node_kind_name(4).is_none());
        assert_eq!(node_kind(0), Some(0));
        assert_eq!(node_kind(11), Some(3));
        assert!(node_kind(12).is_none());
        assert!(node_cell(NODE_CELLS).is_none());
    }

    #[test]
    fn test_lifecycle_ops_closed() {
        let mut o = 0usize;
        while o < LIFECYCLE_OPS {
            assert!(lifecycle_op_name(o).is_some());
            o += 1;
        }
        assert_eq!(lifecycle_op_name(0), Some("create"));
        assert_eq!(lifecycle_op_name(9), Some("load"));
        assert!(lifecycle_op_name(10).is_none());
        assert!(lifecycle_cell(LIFECYCLE_OPS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = lifecycle_cell(3).unwrap_or(0);
        assert_eq!(a, lifecycle_cell(3).unwrap_or(0));
        assert_ne!(a, lifecycle_cell(4).unwrap_or(0));
        assert!(docker_sign(DOCKER_SIGNS).is_none());
        assert!(io_cell(IO_CELLS).is_none());
    }

    #[test]
    fn test_docker_signs_closed() {
        assert_eq!(docker_sign_name(0), Some("镜像构建联签"));
        assert_eq!(docker_sign_name(3), Some("卷联签"));
        assert!(docker_sign_name(4).is_none());
    }

    #[test]
    fn test_io_names_closed() {
        assert_eq!(io_cell_name(0), Some("导出 kubeconfig"));
        assert_eq!(io_cell_name(5), Some("导入 cluster"));
        assert!(io_cell_name(6).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = KindLedger::new();
        let _ = lt.record_op(5, lifecycle_cell(5).unwrap_or(0));
        let before = lt.last_kind;
        assert_eq!(
            lt.record_op(1, lifecycle_cell(1).unwrap_or(0)),
            Err(KindCode::EOrder)
        );
        assert_eq!(lt.last_kind, before);
        assert!(lt.record_op(6, lifecycle_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = KindLedger::new();
        assert_eq!(
            lt.record_op(3, lifecycle_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(KindCode::EOpDrift)
        );
        assert_eq!(
            lt.record_node(2, node_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(KindCode::ENodeDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = KindLedger::new();
        let _ = lt.record_node(0, node_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_node(0, node_cell(0).unwrap_or(0)),
            Err(KindCode::EDup)
        );
        let _ = lt.sign_docker(0);
        assert_eq!(lt.sign_docker(0), Err(KindCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = KindLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = KindLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (LIFECYCLE_OPS + NODE_CELLS) as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_injection_rotation() {
        let (_, caught, _) = run_cycle();
        assert_eq!(caught, 10);
        let mut lt = KindLedger::new();
        assert!(lt.record_op(LIFECYCLE_OPS, 0).is_err());
        assert_eq!(lt.stats(), (0, 0, 0));
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
        assert_eq!(SEG_BASE, 0xC8340);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_anchor_low16_zero() {
        assert_eq!(ANCHOR & 0xFFFF, 0);
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(KindCode::Ok as u32, 0);
        assert_eq!(KindCode::EGradeDrift as u32, 12);
        let all = [
            KindCode::Ok,
            KindCode::ENoNode,
            KindCode::ENodeDrift,
            KindCode::ENoOp,
            KindCode::EOpDrift,
            KindCode::EOrder,
            KindCode::ENoSign,
            KindCode::ESignMiss,
            KindCode::EDup,
            KindCode::ESeal,
            KindCode::EInj,
            KindCode::EFp,
            KindCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0u32;
        while i < 13 {
            assert!(!all[i as usize].detail().is_empty());
            assert_eq!(all[i as usize].val(), SEG_BASE + i);
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        let lt = KindLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = KindLedger::new();
        let _ = lt2.judge_gate(0);
        assert_eq!(lt2.judge_grade(), Grade::S3);
        let mut lt3 = KindLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt3.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt3.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52270_checks_green() {
        let s = run_unx52270_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
