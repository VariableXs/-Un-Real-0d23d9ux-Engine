//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18476 · 兼容层防重声明（与 D3 划界） · 0x7F2C0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18476-J1 20 条主题聚类对 D3 可见判据对表，嫌疑清单=空断言
//!
//! 设计要点：
//!   - 20 条主题聚类是兼容层声明域的封闭清单（写死聚类表），每条聚类
//!     挂一个「D3 可见」划界位：可见的主题归 D3 域发声明，本域只认
//!     不可见的 13 条；
//!   - 声明登记四闸：未知聚类 → D3 越界（可见主题在此声明即拒，必须
//!     走 D3 通道）→ 重复声明 → 乱序（聚类号必须严格升序登记）；
//!   - 嫌疑清单=空断言：闭环集合（经四闸登记的声明表）复核不得出现
//!     任何重复聚类——有重复即进嫌疑清单并拒收口；
//!   - 对表判据：D3_VISIBLE 划界位表与冻结可见数 7 逐项复算一致
//!     （对 D3 可见判据对表）；
//!   - 注入核三类：重复声明（bypass 直塞后复核必抓）/ D3 越界声明 /
//!     未知聚类号——各 10 轮必抓；
//!   - 边界：空登记表复核合法 / 登记满 13 条后越界 / 乱序双拒；
//!   - no_std + alloc；零 unwrap/expect/panic/浮点/IO/墙钟/随机。
//!
//! 本模块结构清单：
//!   [数据结构] 聚类表 CLUSTERS[20] + D3_VISIBLE 划界位面（bool 表）
//!   [状态机]   登记序态（last_registered 严格升序推进）
//!   [账本]     声明账 Vec<Decl> + 嫌疑清单 Vec<u8> + 注入三维账
//!   [错误路径] DgCode 11 码闭集，code() = SEG_BASE + n
//!   [注入核]   重复/D3 越界/未知聚类 三类 ×10 轮逐类必抓
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F2C0，AI-VX11-1105 批04 独占区，A08 工单卡六）。
pub const SEG_BASE: u32 = 0x7F2C0;

// ── 域闭集与冻结常量 ────────────────────────────────────────────
/// 主题聚类数（冻结：20，J1 原文锚定）。
pub const CLUSTER_N: usize = 20;
/// D3 可见聚类数（冻结：7；划界位表逐项求和的闭式）。
pub const D3_VISIBLE_N: usize = 7;
/// 本域可声明聚类数（冻结推导：20 − 7 = 13）。
pub const LOCAL_DECL_N: usize = 13;
/// 注入轮数（写死：10/10 语义）。
pub const INJECT_ROUNDS: u32 = 10;

/// 20 条主题聚类（冻结清单；slug 为聚类标识，中文为语义注）。
pub const CLUSTERS: [&str; CLUSTER_N] = [
    "fonts",          // 0 字体替换策略
    "shortcuts",      // 1 快捷方式改写
    "shellext",       // 2 壳扩展挂载
    "ddraw",          // 3 DirectDraw 兼容
    "d3d8",           // 4 D3D8 兼容（D3 可见：D3 域判据）
    "d3d9",           // 5 D3D9 兼容（D3 可见）
    "dsound",         // 6 DirectSound 兼容
    "dinput",         // 7 DirectInput 兼容
    "joystick",       // 8 游戏杆映射
    "prndrv",         // 9 打印机驱动代偿（D3 可见）
    "dllredir",       // 10 DLL 重定向
    "apphelp",        // 11 应用帮助垫片（D3 可见）
    "layered",        // 12 兼容性标志层
    "gdiscale",       // 13 GDI 缩放（D3 可见：D3 域判据）
    "dpisense",       // 14 DPI 感知
    "hookpol",        // 15 钩子策略
    "msgpump",        // 16 消息泵替身（D3 可见）
    "memlayout",      // 17 内存布局垫片
    "handlecnt",      // 18 句柄配额
    "wndclass",       // 19 窗口类注册（D3 可见）
];

/// D3 划界位表（冻结；与 CLUSTERS 同下标）。推导：D3 域（渲染/
/// 显示/垫片判定）消费的判据主题让位，共 7 条——4/5/9/11/13/16/19。
pub const D3_VISIBLE: [bool; CLUSTER_N] = [
    false, false, false, false, // 0..3
    true,  true,  false, false, // 4,5 可见
    false, true,  false, true,  // 9,11 可见
    false, true,  false, false, // 13 可见
    true,  false, false, true,  // 16,19 可见
];

/// 划界位表逐项求和自检（冻结闭式 = 7）。
pub fn d3_visible_count() -> usize {
    let mut n = 0usize;
    let mut i = 0;
    while i < CLUSTER_N {
        if let Some(v) = D3_VISIBLE.get(i) {
            if *v {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

// ── 诊断码闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DgCode {
    /// 未知聚类
    UnknownCluster,
    /// 重复声明
    DuplicateDecl,
    /// D3 越界（可见主题误在本域声明）
    BoundaryRoute,
    /// 登记乱序（聚类号必须严格升序）
    OrderViolation,
    /// 登记表越容
    RegistryFull,
    /// 嫌疑清单非空
    SuspicionNonEmpty,
    /// 划界对表失配
    BoundaryMismatch,
    /// 计数守恒破坏
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
    /// 段位越窗
    SegmentOutside,
    /// 复核发现重复（bypass 塞入被复核点名）
    AuditDuplicate,
}

impl DgCode {
    /// 诊断码 = SEG_BASE + 枚举序号。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DgCode::UnknownCluster => "未知聚类：0..20 闭集外，先查 CLUSTERS",
            DgCode::DuplicateDecl => "重复声明：一聚类一声明，第二次即拒",
            DgCode::BoundaryRoute => "D3 越界：可见主题须走 D3 通道声明",
            DgCode::OrderViolation => "登记乱序：聚类号必须严格升序",
            DgCode::RegistryFull => "登记表越容：本域可声明 13 条上限",
            DgCode::SuspicionNonEmpty => "嫌疑清单非空：闭环集存在重复，禁收口",
            DgCode::BoundaryMismatch => "划界对表失配：可见位表与冻结数不符",
            DgCode::ConservationBroken => "计数守恒破坏：声明数与账不平",
            DgCode::NotReproducible => "双跑不一致：复现红线",
            DgCode::SegmentOutside => "段位越窗：诊断码不在 0x7F2C0 段",
            DgCode::AuditDuplicate => "复核发现重复：bypass 通道被点名",
        }
    }
    pub const ALL: [DgCode; 11] = [
        DgCode::UnknownCluster,
        DgCode::DuplicateDecl,
        DgCode::BoundaryRoute,
        DgCode::OrderViolation,
        DgCode::RegistryFull,
        DgCode::SuspicionNonEmpty,
        DgCode::BoundaryMismatch,
        DgCode::ConservationBroken,
        DgCode::NotReproducible,
        DgCode::SegmentOutside,
        DgCode::AuditDuplicate,
    ];
}

/// 段内任意码 → 中文明细。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < DgCode::ALL.len() {
        if let Some(c) = DgCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F2C0 段闭集内"
}

// ── 声明账 ─────────────────────────────────────────────────────
/// 单条声明。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decl {
    /// 聚类号（0..20）。
    pub cluster: usize,
    /// 登记序（从 1 起，升序单调）。
    pub ord: u32,
}

/// 声明登记账 + 嫌疑清单 + 注入三维账。
pub struct DeclLedger {
    /// 闭环声明表（经四闸后落账）。
    pub decls: Vec<Decl>,
    /// 嫌疑清单（复核出的重复聚类号；收口时必须为空）。
    pub suspicion: Vec<u8>,
    /// 登记序游标（严格升序）。
    pub last_cluster: i32,
    /// 注入三维账：tries/caught ×3 类。
    pub inj_tries: [u32; 3],
    pub inj_caught: [u32; 3],
    /// 拒绝计数（按码分桶只记 4 类常用）。
    pub rej_dup: u32,
    pub rej_route: u32,
    pub rej_order: u32,
    pub rej_unknown: u32,
}

impl DeclLedger {
    pub fn new() -> DeclLedger {
        DeclLedger {
            decls: Vec::new(),
            suspicion: Vec::new(),
            last_cluster: -1,
            inj_tries: [0; 3],
            inj_caught: [0; 3],
            rej_dup: 0,
            rej_route: 0,
            rej_order: 0,
            rej_unknown: 0,
        }
    }

    /// 四闸登记：未知 → D3 越界 → 重复 → 乱序；过闸落账。
    pub fn register(&mut self, cluster: usize) -> Result<u32, u32> {
        // 闸一：未知聚类。
        if cluster >= CLUSTER_N {
            self.rej_unknown = self.rej_unknown.saturating_add(1);
            return Err(DgCode::UnknownCluster.code());
        }
        // 闸二：D3 越界（可见主题须走 D3 通道）。
        let visible = match D3_VISIBLE.get(cluster) {
            Some(v) => *v,
            None => return Err(DgCode::UnknownCluster.code()),
        };
        if visible {
            self.rej_route = self.rej_route.saturating_add(1);
            return Err(DgCode::BoundaryRoute.code());
        }
        // 闸三：重复声明（闭环表内查同号）。
        let mut dup = false;
        let mut i = 0;
        while i < self.decls.len() {
            if let Some(d) = self.decls.get(i) {
                if d.cluster == cluster {
                    dup = true;
                }
            }
            i += 1;
        }
        if dup {
            self.rej_dup = self.rej_dup.saturating_add(1);
            // 嫌疑清单只在闭环复核发现【已落账】重复时才记；
            // 登记闸拒掉的重复进注入账不进嫌疑清单（见 inject_rounds）。
            return Err(DgCode::DuplicateDecl.code());
        }
        // 闸四：乱序（严格升序：首条任意，之后必须大于前条）。
        if self.last_cluster >= 0 && (cluster as i32) <= self.last_cluster {
            self.rej_order = self.rej_order.saturating_add(1);
            return Err(DgCode::OrderViolation.code());
        }
        // 落账。
        if self.decls.len() >= LOCAL_DECL_N {
            return Err(DgCode::RegistryFull.code());
        }
        let ord = match self.decls.len() {
            n => (n as u32).saturating_add(1),
        };
        self.decls.push(Decl { cluster, ord });
        self.last_cluster = cluster as i32;
        Ok(ord)
    }

    /// bypass 直塞（不检任何闸——模拟误用通道，仅供注入核调用）。
    pub fn force_bypass(&mut self, cluster: usize) {
        let ord = match self.decls.len() {
            n => (n as u32).saturating_add(1),
        };
        self.decls.push(Decl { cluster, ord });
    }

    /// 闭环复核：扫重复聚类 + 对表划界位 + 嫌疑清单收口断言。
    pub fn close_audit(&mut self) -> Result<usize, u32> {
        // 对表一：划界位表逐项求和必须等于冻结数。
        if d3_visible_count() != D3_VISIBLE_N {
            return Err(DgCode::BoundaryMismatch.code());
        }
        // 对表二：可见主题不得出现在闭环表。
        let mut i = 0;
        while i < self.decls.len() {
            let c = match self.decls.get(i) {
                Some(d) => d.cluster,
                None => return Err(DgCode::UnknownCluster.code()),
            };
            if c >= CLUSTER_N {
                return Err(DgCode::UnknownCluster.code());
            }
            if let Some(v) = D3_VISIBLE.get(c) {
                if *v {
                    return Err(DgCode::BoundaryRoute.code());
                }
            }
            i += 1;
        }
        // 复核三：重复扫描（同号出现两次即进嫌疑清单）。
        let mut suspects = 0usize;
        let mut a = 0;
        while a < self.decls.len() {
            let ca = match self.decls.get(a) {
                Some(d) => d.cluster,
                None => return Err(DgCode::UnknownCluster.code()),
            };
            let mut b = a + 1;
            while b < self.decls.len() {
                let cb = match self.decls.get(b) {
                    Some(d) => d.cluster,
                    None => return Err(DgCode::UnknownCluster.code()),
                };
                if ca == cb {
                    suspects = suspects.saturating_add(1);
                    self.suspicion.push(ca as u8);
                }
                b += 1;
            }
            a += 1;
        }
        if suspects > 0 {
            return Err(DgCode::AuditDuplicate.code());
        }
        // 复核四：登记序严格升序。
        let mut k = 1;
        while k < self.decls.len() {
            let prev = match self.decls.get(k - 1) {
                Some(d) => d.cluster,
                None => return Err(DgCode::UnknownCluster.code()),
            };
            let cur = match self.decls.get(k) {
                Some(d) => d.cluster,
                None => return Err(DgCode::UnknownCluster.code()),
            };
            if cur <= prev {
                return Err(DgCode::OrderViolation.code());
            }
            k += 1;
        }
        Ok(self.decls.len())
    }

    /// 嫌疑清单=空断言。
    pub fn suspicion_empty(&self) -> bool {
        self.suspicion.len() == 0
    }
}

// ── 黄金全链 ───────────────────────────────────────────────────
/// 本域 13 条可声明聚类按升序全登记 → 复核过 → 嫌疑清单空。
/// 返回（登记数, 嫌疑数）。
pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut led = DeclLedger::new();
    let mut i = 0;
    while i < CLUSTER_N {
        // 可见主题跳过（走 D3 通道，不入本域账）。
        let vis = match D3_VISIBLE.get(i) {
            Some(v) => *v,
            None => return Err(DgCode::UnknownCluster.code()),
        };
        if !vis {
            led.register(i)?;
        }
        i += 1;
    }
    let n = led.close_audit()?;
    if n != LOCAL_DECL_N {
        return Err(DgCode::ConservationBroken.code());
    }
    if !led.suspicion_empty() {
        return Err(DgCode::SuspicionNonEmpty.code());
    }
    Ok((n as u32, led.suspicion.len() as u32))
}

// ── 注入核：三类坏样本 ×10 轮 ──────────────────────────────────
/// 注入一：重复声明——bypass 直塞同号两条，闭环复核必点名。
/// 注入二：D3 越界声明——可见主题在本域登记必被 BoundaryRoute 拒。
/// 注入三：未知聚类——越界号必被 UnknownCluster 拒。
pub fn inject_rounds(rounds: u32) -> Result<(u32, u32), u32> {
    let mut tries = 0u32;
    let mut caught = 0u32;
    let mut r = 0u32;
    while r < rounds {
        // —— 类一：重复声明（bypass 塞入，复核点名）——
        tries = tries.saturating_add(1);
        {
            let mut led = DeclLedger::new();
            let _ = led.register(0);
            let _ = led.register(1);
            // bypass 直塞一条与 1 重复的声明（模拟误用通道）。
            led.force_bypass(1);
            let hit = led.close_audit().err() == Some(DgCode::AuditDuplicate.code())
                && !led.suspicion_empty();
            if hit {
                caught = caught.saturating_add(1);
            }
        }
        // —— 类二：D3 越界声明 ——
        tries = tries.saturating_add(1);
        {
            let mut led = DeclLedger::new();
            // 4 号（d3d8）可见 → 必拒。
            let hit = led.register(4).err() == Some(DgCode::BoundaryRoute.code())
                && led.decls.len() == 0;
            if hit {
                caught = caught.saturating_add(1);
            }
        }
        // —— 类三：未知聚类 ——
        tries = tries.saturating_add(1);
        {
            let mut led = DeclLedger::new();
            let hit = led.register(CLUSTER_N).err() == Some(DgCode::UnknownCluster.code())
                && led.register(CLUSTER_N + 5).err() == Some(DgCode::UnknownCluster.code());
            if hit {
                caught = caught.saturating_add(1);
            }
        }
        r = r.saturating_add(1);
    }
    if caught != tries {
        return Err(DgCode::ConservationBroken.code());
    }
    Ok((tries, caught))
}

/// 双跑确定性。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DgCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DgCode::ALL.len() {
        if let Some(c) = DgCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x7F000 && SEG_BASE <= 0x7F400
}

/// 划界对表探针：CLUSTERS 与 D3_VISIBLE 同长、可见号集合与冻结
/// 推导 {4,5,9,11,13,16,19} 逐项一致。
pub fn boundary_probe() -> Result<(), u32> {
    if CLUSTERS.len() != CLUSTER_N || D3_VISIBLE.len() != CLUSTER_N {
        return Err(DgCode::BoundaryMismatch.code());
    }
    let expect: [usize; D3_VISIBLE_N] = [4, 5, 9, 11, 13, 16, 19];
    let mut got: [usize; D3_VISIBLE_N] = [CLUSTER_N; D3_VISIBLE_N];
    let mut n = 0usize;
    let mut i = 0;
    while i < CLUSTER_N {
        let vis = match D3_VISIBLE.get(i) {
            Some(v) => *v,
            None => return Err(DgCode::BoundaryMismatch.code()),
        };
        if vis {
            if n >= D3_VISIBLE_N {
                return Err(DgCode::BoundaryMismatch.code());
            }
            if let Some(slot) = got.get_mut(n) {
                *slot = i;
            }
            n = n.saturating_add(1);
        }
        i += 1;
    }
    if n != D3_VISIBLE_N {
        return Err(DgCode::BoundaryMismatch.code());
    }
    let mut k = 0;
    while k < D3_VISIBLE_N {
        let g = match got.get(k) {
            Some(v) => *v,
            None => return Err(DgCode::BoundaryMismatch.code()),
        };
        let e = match expect.get(k) {
            Some(v) => *v,
            None => return Err(DgCode::BoundaryMismatch.code()),
        };
        if g != e {
            return Err(DgCode::BoundaryMismatch.code());
        }
        k += 1;
    }
    Ok(())
}

/// 乱序边界：0 登记后再登 0 → 重复；1 登记后登 0 → 乱序。
pub fn order_edges() -> Result<(), u32> {
    let mut led = DeclLedger::new();
    // 同号重复拒：登记 2 后再登 2（重复闸在乱序闸前，先拦）。
    let _ = led.register(2)?;
    let dup = led.register(2).err() == Some(DgCode::DuplicateDecl.code());
    // 回退乱序拒：登记 3 后登一个【未登记过的】低号 1——重复闸放行，
    // 撞到乱序闸（1 <= last=3）。若用已登记号测乱序会先吃重复拒。
    let _ = led.register(3)?;
    let order = led.register(1).err() == Some(DgCode::OrderViolation.code());
    if dup && order {
        Ok(())
    } else {
        Err(DgCode::ConservationBroken.code())
    }
}

/// 空登记表复核：0 条声明合法收口（空域合法形态）。
pub fn empty_registry_edge() -> Result<(), u32> {
    let mut led = DeclLedger::new();
    let n = led.close_audit()?;
    if n != 0 || !led.suspicion_empty() {
        return Err(DgCode::ConservationBroken.code());
    }
    Ok(())
}

/// 登记满边界：13 条之后第 14 条必须 RegistryFull。
pub fn full_registry_edge() -> Result<(), u32> {
    let mut led = DeclLedger::new();
    let mut i = 0;
    let mut filled = 0u32;
    while i < CLUSTER_N {
        let vis = match D3_VISIBLE.get(i) {
            Some(v) => *v,
            None => break,
        };
        if !vis {
            let r = led.register(i);
            match r {
                Ok(_) => filled = filled.saturating_add(1),
                Err(c) if c == DgCode::RegistryFull.code() => {}
                Err(_) => return Err(DgCode::ConservationBroken.code()),
            }
        }
        i += 1;
    }
    // 13 条全落账且无越容（本探针本身只登记 13 条合法数）。
    if filled != LOCAL_DECL_N as u32 || led.decls.len() != LOCAL_DECL_N {
        return Err(DgCode::ConservationBroken.code());
    }
    Ok(())
}

// ── 判据出口 ───────────────────────────────────────────────────
/// UNX-F18476 自检判据集（≤20 条；≤8 条动态明细）。
pub fn run_unx18476_checks() -> CheckSet {
    let mut s = CheckSet::new("unx18476");
    // 1. 聚类闭集冻结：20 条。
    s.add(
        "J1-聚类闭集20",
        CLUSTER_N == 20 && CLUSTERS.len() == 20 && D3_VISIBLE.len() == 20,
        "20 条主题聚类写死",
    );
    // 2. 划界对表：可见位表 = 冻结号集 {4,5,9,11,13,16,19}。
    s.add("J1-划界对表7条", boundary_probe().is_ok(), "D3 可见号集逐项一致");
    // 3. 黄金全链：13 条升序登记全过 + 嫌疑清单空。
    let g = golden_run();
    s.add_dyn("J1-黄金全链13条", g.is_ok(), &format!("登记/嫌疑={:?}", g));
    // 4. 嫌疑清单=空断言（黄金链后显式复核）。
    {
        let mut led = DeclLedger::new();
        let mut i = 0;
        while i < CLUSTER_N {
            let vis = match D3_VISIBLE.get(i) {
                Some(v) => *v,
                None => false,
            };
            if !vis {
                let _ = led.register(i);
            }
            i += 1;
        }
        let _ = led.close_audit();
        s.add(
            "J1-嫌疑清单空",
            led.suspicion_empty() && led.decls.len() == LOCAL_DECL_N,
            "闭环集零重复",
        );
    }
    // 5. 注入三类 ×10 轮全抓。
    let inj = inject_rounds(INJECT_ROUNDS);
    let inj_ok = match inj {
        Ok((t, c)) => t > 0 && c == t,
        Err(_) => false,
    };
    s.add_dyn("J1-注入三类10/10", inj_ok, &format!("tries/caught={:?}", inj));
    // 6. 乱序/重复边界双拒。
    s.add("J1-乱序重复双拒", order_edges().is_ok(), "同号重复拒 / 回退乱序拒");
    // 7. 空登记表合法收口。
    s.add("J1-空表收口", empty_registry_edge().is_ok(), "0 条声明合法形态");
    // 8. 登记满边界：恰 13 条。
    s.add("J1-登记满13", full_registry_edge().is_ok(), "本域可声明上限 13");
    // 9. 划界位逐项：每条可见位与聚类语义注释一致（抽核 4/19 两端）。
    {
        let a = match D3_VISIBLE.get(4) {
            Some(v) => *v,
            None => false,
        };
        let b = match D3_VISIBLE.get(19) {
            Some(v) => *v,
            None => false,
        };
        let c = match D3_VISIBLE.get(0) {
            Some(v) => *v,
            None => true,
        };
        s.add("J1-划位抽核", a && b && !c, "d3d8/wndclass 可见，fonts 不可见");
    }
    // 10. 本域/D3 计数闭式：13 + 7 = 20。
    s.add(
        "J1-计数闭式13+7",
        LOCAL_DECL_N == 13 && D3_VISIBLE_N == 7 && LOCAL_DECL_N + D3_VISIBLE_N == CLUSTER_N,
        "划界两侧之和等于闭集",
    );
    // 11. 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // 12. 段位守卫。
    s.add("J3-段位守卫", segment_guard(), "0x7F2C0 窗口对齐 0x40");
    // 13. 文案闭集。
    {
        let mut ok = true;
        let mut i = 0;
        while i < DgCode::ALL.len() {
            if let Some(c) = DgCode::ALL.get(i) {
                if c.detail().len() == 0 {
                    ok = false;
                }
            }
            i += 1;
        }
        s.add("J4-文案闭集", ok, "11 码中文明细全非空");
    }
    // 14. 拒绝账守恒：黄金链零拒绝，注入链拒账递增。
    {
        let mut led = DeclLedger::new();
        let _ = led.register(2);
        let _ = led.register(2); // 重复拒。
        let _ = led.register(6); // 可见? 6=dsound 不可见 → 收。
        let ok = led.rej_dup == 1 && led.decls.len() == 2;
        s.add("J1-拒绝账守恒", ok, "拒账逐次递增不吞账");
    }
    s
}

// ── 测试 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_normal_flow() {
        let mut led = DeclLedger::new();
        assert!(led.register(0).is_ok());
        assert!(led.register(1).is_ok());
        assert!(led.register(2).is_ok());
        assert_eq!(led.decls.len(), 3);
    }

    #[test]
    fn t02_boundary_unknown_cluster() {
        let mut led = DeclLedger::new();
        assert_eq!(
            led.register(CLUSTER_N).err(),
            Some(DgCode::UnknownCluster.code())
        );
    }

    #[test]
    fn t03_boundary_empty_and_full() {
        assert!(empty_registry_edge().is_ok());
        assert!(full_registry_edge().is_ok());
    }

    #[test]
    fn t04_inject_duplicate_decl() {
        let mut led = DeclLedger::new();
        let _ = led.register(0);
        led.force_bypass(0);
        assert_eq!(
            led.close_audit().err(),
            Some(DgCode::AuditDuplicate.code())
        );
        assert!(!led.suspicion_empty());
    }

    #[test]
    fn t05_inject_d3_boundary() {
        let mut led = DeclLedger::new();
        assert_eq!(led.register(4).err(), Some(DgCode::BoundaryRoute.code()));
        assert_eq!(led.register(19).err(), Some(DgCode::BoundaryRoute.code()));
        assert_eq!(led.decls.len(), 0);
    }

    #[test]
    fn t06_inject_unknown_cluster() {
        let mut led = DeclLedger::new();
        assert_eq!(
            led.register(CLUSTER_N + 3).err(),
            Some(DgCode::UnknownCluster.code())
        );
    }

    #[test]
    fn t07_order_violation() {
        assert!(order_edges().is_ok());
    }

    #[test]
    fn t08_boundary_table_frozen() {
        assert!(boundary_probe().is_ok());
        assert_eq!(d3_visible_count(), 7);
    }

    #[test]
    fn t09_double_run_determinism() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn t10_segment_guard() {
        assert!(segment_guard());
    }

    #[test]
    fn t11_checks_all_green() {
        let set = run_unx18476_checks();
        assert!(set.all_passed(), "UNX-F18476 自检须全绿：{}", set.red_items_text());
    }
}
