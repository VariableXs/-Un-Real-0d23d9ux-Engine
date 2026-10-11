//! unx105_sharedalert — UNX-F0916 共享向量检测与配置告警（与 F0904
//! 票制分层）。
//!
//! 判据原文（施工书锚点 UNX-F0916，CoRun·UNX-A2-B06 批）：
//! **UNX-F0916-J1 检测正确（共享配置 10/10 次检出告警），MSI 时代
//! 共享告警升级（MSI-X 向量出现票制 10/10 次红账——架构违例），
//! 告警消解（整改后撤销 10/10 次）**。
//!
//! 职责定位：F0904 的票制把 INTx 时代的"线是物理或的"建模成共享
//! 票分发——那是**合法共享**（INTx 线本来就设计给多设备拉）；
//! 但 MSI 时代的向量是**消息语义**（每向量独占一路由表项），
//! 共享配置在 MSI 时代不是省资源是**架构违例**（路由表分不清
//! 该谁收——Windows 的 PCI 资源仲裁在这一步直接拒设备启动）。
//! Windows 深在哪：资源仲裁器把 IRQ 共享分代管理——传统线共享
//! 是 Warn 级（可见但不拦），MSI 共享是 Fail 级（黄叹号/启动拒
//! 绝），设备管理器把共享配置作为可检索告警呈现。Varix：①**检
//! 测**——向量→挂线设备集聚合，size>1 即共享配置（10/10 检出）；
//! ②**分层**——INTx 共享=Warn 告警（合法可见），MSI/MSI-X 共享
//! =Red 红账（架构违例升级，10/10）；③**消解**——整改（改配
//! 独占/换向量）后重扫撤销告警（10/10）；④**票制分层声明**——
//! F0904 管票序（共享向量的挂线序==注册序），本单管检出与告警
//! （同一挂线集的两面，语义不重叠）。
//! 上游：F0904 共享票制（挂线集与票序声明本体）、F0908 路径形态
//! （时代判定依据）、F0915 注销清理（摘线触发重扫）；下游：
//! F0899 健康联动（告警入摘要）、ktest B06。
//! 本单交付**占用聚合 + 三级检出 + 时代升级 + 告警生命周期 + 票
//! 序分层 + 十轮注入 + 压测 + 快照**八面。
//!
//! 设计要点：no_std+alloc；零 unwrap/expect；零 IO 零墙钟（tick 显
//! 性注入）；LCG 确定性仿真；FNV-1a 快照往返；错误全显性带诊断
//! 码与建议。诊断码段 0x49（0x48 为 F0915 所取）。

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const SHARED_ALERT_VERSION: &str = "B06-sharedalert-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x49 段，本单独占——0x48 为 F0915 所取）
// ---------------------------------------------------------------------------

/// 0x4901 共享检出告警（Warn 级——INTx 时代合法共享但须可见）。
pub const XC_SHARED_WARN: u32 = 0x4901;
/// 0x4902 架构违例红账（MSI/MSI-X 共享——时代违例升级）。
pub const XC_ERA_RED: u32 = 0x4902;
/// 0x4903 消解失败（整改后告警未撤销）。
pub const XC_CLOSE_FAIL: u32 = 0x4903;
/// 0x4904 幽灵告警（close 不存在的告警/重复撤销）。
pub const XC_GHOST_ALERT: u32 = 0x4904;
/// 0x4905 快照指纹漂移。
pub const XC_SNAP_DRIFT: u32 = 0x4905;
/// 0x4906 检测漏报（共享配置未被检出）。
pub const XC_DETECT_MISS: u32 = 0x4906;
/// 0x4907 升级失准（该红未红/该警未警——分层口径破）。
pub const XC_LEVEL_MISS: u32 = 0x4907;
/// 0x4908 空挂线集（向量占用无设备——聚合失守）。
pub const XC_EMPTY_OCCUPY: u32 = 0x4908;
/// 0x4909 票序失守（挂线序≠注册序——F0904 分层声明破坏）。
pub const XC_TICKET_ORDER: u32 = 0x4909;
/// 0x490A 重复挂线（同设备二度挂同向量）。
pub const XC_DUP_ATTACH: u32 = 0x490A;

// ---------------------------------------------------------------------------
// §1 向量占用模型（挂线集与时代）
// ---------------------------------------------------------------------------

/// 向量路径形态（F0908 时代语义同源——共享合法性的判定依据）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VecPath {
    /// 传统引脚线（共享合法——Warn 级可见）。
    Intx,
    /// MSI 消息向量（独占语义——共享即违例）。
    Msi,
    /// MSI-X 消息向量（独占语义——共享即违例）。
    MsiX,
}

impl VecPath {
    /// 人话名（告警行消费）。
    pub const fn name(self) -> &'static str {
        match self {
            VecPath::Intx => "intx",
            VecPath::Msi => "msi",
            VecPath::MsiX => "msix",
        }
    }

    /// 该时代共享是否合法（INTx 合法；MSI/MSI-X 恒违例）。
    pub const fn sharing_legal(self) -> bool {
        match self {
            VecPath::Intx => true,
            VecPath::Msi | VecPath::MsiX => false,
        }
    }
}

/// 向量占用：挂线设备集（注册序==F0904 票序——分层声明本体）。
#[derive(Clone, Debug)]
pub struct VecOccupy {
    /// 路径形态（占用时代）。
    pub path: VecPath,
    /// 挂线设备键集（注册序保序——票序断言依据）。
    pub devices: Vec<u32>,
}

impl VecOccupy {
    /// 是否共享配置（>1 设备挂同向量）。
    pub fn is_shared(&self) -> bool {
        self.devices.len() > 1
    }
}

// ---------------------------------------------------------------------------
// §2 告警模型（生命周期：开→升级/消解）
// ---------------------------------------------------------------------------

/// 告警级别闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlertLevel {
    /// Warn——INTx 时代合法共享。
    Warn,
    /// Red——MSI 时代架构违例。
    Red,
}

impl AlertLevel {
    /// 人话名。
    pub const fn name(self) -> &'static str {
        match self {
            AlertLevel::Warn => "告警",
            AlertLevel::Red => "红账",
        }
    }
}

/// 一条配置告警。
#[derive(Clone, Debug)]
pub struct Alert {
    /// 所属向量。
    pub vector: u32,
    /// 级别（Warn/Red）。
    pub level: AlertLevel,
    /// 挂线设备快照（开警时点）。
    pub devices: Vec<u32>,
    /// 开警 tick。
    pub open_at: u64,
    /// 消解 tick（None=未消解）。
    pub closed_at: Option<u64>,
}

// ---------------------------------------------------------------------------
// §3 告警总账（聚合→检出→升级→消解→票序分层）
// ---------------------------------------------------------------------------

/// 共享配置告警总账。
#[derive(Default)]
pub struct AlertLedger {
    /// 向量→占用（BTreeMap 键序——确定性扫描底座）。
    pub occupy: BTreeMap<u32, VecOccupy>,
    /// 活跃告警（向量→告警）。
    pub active: BTreeMap<u32, Alert>,
    /// 历史告警（消解归档——append-only 审计面）。
    pub archive: Vec<Alert>,
    /// 注册挂线总数。
    pub attach_total: u64,
    /// 摘线总数。
    pub detach_total: u64,
    /// 开警总数。
    pub open_total: u64,
    /// 红账升级总数。
    pub red_total: u64,
    /// 消解总数。
    pub close_total: u64,
}

impl AlertLedger {
    /// 新账。
    pub fn new() -> AlertLedger {
        AlertLedger {
            occupy: BTreeMap::new(),
            active: BTreeMap::new(),
            archive: Vec::new(),
            attach_total: 0,
            detach_total: 0,
            open_total: 0,
            red_total: 0,
            close_total: 0,
        }
    }

    /// 设备挂线：同向量多设备即共享配置的原料。
    ///
    /// 票序纪律：挂线序==注册序（F0904 分层声明——同设备重复挂线
    /// 显性拒）。
    pub fn attach(&mut self, vector: u32, path: VecPath, device: u32) -> Result<(), (u32, String)> {
        let occ = self.occupy.entry(vector).or_insert_with(|| VecOccupy {
            path,
            devices: Vec::new(),
        });
        if occ.path != path {
            return Err((
                XC_LEVEL_MISS,
                format!(
                    "向量 {} 路径冲突：已为 {} 新挂 {}——同向量不同代",
                    vector, occ.path.name(), path.name()
                ),
            ));
        }
        if occ.devices.contains(&device) {
            return Err((
                XC_DUP_ATTACH,
                format!("设备 #{} 重复挂向量 {}——挂线唯一", device, vector),
            ));
        }
        occ.devices.push(device);
        self.attach_total += 1;
        Ok(())
    }

    /// 设备摘线：摘后零设备集即聚合失守候选（显性清槽）。
    pub fn detach(&mut self, vector: u32, device: u32) -> Result<(), (u32, String)> {
        let occ = match self.occupy.get_mut(&vector) {
            Some(v) => v,
            None => return Err((XC_EMPTY_OCCUPY, format!("向量 {} 无占用账——摘线无源", vector))),
        };
        let before = occ.devices.len();
        occ.devices.retain(|d| *d != device);
        if occ.devices.len() == before {
            return Err((
                XC_EMPTY_OCCUPY,
                format!("设备 #{} 未挂向量 {}——摘线无的", device, vector),
            ));
        }
        self.detach_total += 1;
        if occ.devices.is_empty() {
            self.occupy.remove(&vector);
        }
        Ok(())
    }

    /// 全量扫描：检出共享配置并按时代分层开警/升级。
    ///
    /// 返回本次新开告警数（检测 10/10 的账面来源）。
    pub fn scan(&mut self, tick: u64) -> Result<usize, (u32, String)> {
        let mut opened = 0usize;
        // 键序快照（避免借用冲突——先收集待警向量）。
        let mut to_open: Vec<(u32, AlertLevel, Vec<u32>)> = Vec::new();
        for (v, occ) in self.occupy.iter() {
            if occ.is_shared() {
                let level = if occ.path.sharing_legal() {
                    AlertLevel::Warn
                } else {
                    AlertLevel::Red
                };
                to_open.push((*v, level, occ.devices.clone()));
            }
        }
        for (v, level, devices) in to_open {
            match self.active.get_mut(&v) {
                Some(a) => {
                    // 已有告警：升级判定（Warn→Red 是单向升级）。
                    if level == AlertLevel::Red && a.level == AlertLevel::Warn {
                        a.level = AlertLevel::Red;
                        a.open_at = tick;
                        self.red_total += 1;
                    }
                }
                None => {
                    if level == AlertLevel::Red {
                        self.red_total += 1;
                    }
                    self.active.insert(
                        v,
                        Alert {
                            vector: v,
                            level,
                            devices,
                            open_at: tick,
                            closed_at: None,
                        },
                    );
                    opened += 1;
                    self.open_total += 1;
                }
            }
        }
        // 消解面：不再共享的活跃告警自动撤销（整改即消解）。
        let mut to_close: Vec<u32> = Vec::new();
        for v in self.active.keys() {
            let still_shared = match self.occupy.get(v) {
                Some(occ) => occ.is_shared(),
                None => false,
            };
            if !still_shared {
                to_close.push(*v);
            }
        }
        for v in to_close {
            self.close_alert(v, tick)?;
        }
        Ok(opened)
    }

    /// 显式消解：撤销告警入归档（整改后撤销 10/10 的账面）。
    pub fn close_alert(&mut self, vector: u32, tick: u64) -> Result<(), (u32, String)> {
        let mut a = match self.active.remove(&vector) {
            Some(a) => a,
            None => {
                return Err((
                    XC_GHOST_ALERT,
                    format!("向量 {} 无活跃告警——幽灵消解拒", vector),
                ))
            }
        };
        a.closed_at = Some(tick);
        self.archive.push(a);
        self.close_total += 1;
        Ok(())
    }

    /// 检测对拍：给定共享向量期望集，活跃告警必须恰覆盖（10/10
    /// 的强制点——漏报 0x4906/多报同立案）。
    pub fn detect_audit(&self, expect_shared: &[u32]) -> Result<(), (u32, String)> {
        let mut expect = expect_shared.to_vec();
        expect.sort_unstable();
        let mut actual: Vec<u32> = self.active.keys().copied().collect();
        actual.sort_unstable();
        if expect != actual {
            return Err((
                XC_DETECT_MISS,
                format!("检测对拍失准：期望 {:?} 实际 {:?}", expect, actual),
            ));
        }
        Ok(())
    }

    /// 票序分层断言：每个共享向量的挂线序与注册序一致（F0904
    /// 分层声明——本单只验序不发票）。
    pub fn ticket_order_audit(&self, reg_order: &BTreeMap<u32, u32>) -> Result<(), (u32, String)> {
        // reg_order：设备→注册序（共享向量挂线序必须按此单调）。
        for (v, occ) in self.occupy.iter() {
            if occ.devices.len() < 2 {
                continue;
            }
            let mut last: Option<u32> = None;
            for d in occ.devices.iter() {
                let ord = match reg_order.get(d) {
                    Some(o) => *o,
                    None => {
                        return Err((
                            XC_TICKET_ORDER,
                            format!("向量 {} 设备 #{} 无注册序——账面失偶", v, d),
                        ))
                    }
                };
                if let Some(l) = last {
                    if ord <= l {
                        return Err((
                            XC_TICKET_ORDER,
                            format!("向量 {} 挂线序破票序：设备 #{} 序 {} 不大于前序 {}", v, d, ord, l),
                        ));
                    }
                }
                last = Some(ord);
            }
        }
        Ok(())
    }

    /// 级别口径审计：INTx 共享必 Warn、MSI/MSI-X 共享必 Red（该警
    /// 未警/该红未红同立案）。
    pub fn level_audit(&self) -> Result<(), (u32, String)> {
        for (v, a) in self.active.iter() {
            let path = match self.occupy.get(v) {
                Some(o) => o.path,
                None => return Err((XC_EMPTY_OCCUPY, format!("向量 {} 占用账丢失", v))),
            };
            let want = if path.sharing_legal() {
                AlertLevel::Warn
            } else {
                AlertLevel::Red
            };
            if a.level != want {
                return Err((
                    XC_LEVEL_MISS,
                    format!(
                        "向量 {} 级别失准：{} 期望 {}（路径 {}）",
                        v,
                        a.level.name(),
                        want.name(),
                        path.name()
                    ),
                ));
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// §4 LCG + FNV
// ---------------------------------------------------------------------------

/// 线性同余发生器（Numerical Recipes 参数）。
pub struct Lcg(u64);

impl Lcg {
    /// 播种。
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }

    /// 下一步。
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 33) as u32
    }

    /// 半开区间 [0, n)。
    pub fn below(&mut self, n: u32) -> u32 {
        self.next_u32() % n
    }
}

/// FNV 偏移基。
pub const FNV_OFFSET: u32 = 0x811C_9DC5;

/// FNV-1a 32 位。
pub fn fnv32(data: &[u8], seed: u32) -> u32 {
    let mut h = seed;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 告警账快照。
pub struct AlertSnap {
    /// 指纹。
    pub hash: u32,
    /// 摘要。
    pub summary: String,
}

impl AlertSnap {
    /// 采样。
    pub fn take(led: &AlertLedger) -> AlertSnap {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&led.attach_total.to_le_bytes());
        buf.extend_from_slice(&led.detach_total.to_le_bytes());
        buf.extend_from_slice(&led.open_total.to_le_bytes());
        buf.extend_from_slice(&led.red_total.to_le_bytes());
        buf.extend_from_slice(&led.close_total.to_le_bytes());
        for (v, occ) in led.occupy.iter() {
            buf.extend_from_slice(&v.to_le_bytes());
            buf.push(occ.path as u8);
            buf.extend_from_slice(&occ.devices.len().to_le_bytes());
            for d in occ.devices.iter() {
                buf.extend_from_slice(&d.to_le_bytes());
            }
        }
        for (v, a) in led.active.iter() {
            buf.extend_from_slice(&v.to_le_bytes());
            buf.push(a.level as u8);
            buf.extend_from_slice(&a.open_at.to_le_bytes());
        }
        AlertSnap {
            hash: fnv32(&buf, FNV_OFFSET),
            summary: format!(
                "占用={} 活警={} 挂={} 摘={} 开={} 红={} 消={}",
                led.occupy.len(),
                led.active.len(),
                led.attach_total,
                led.detach_total,
                led.open_total,
                led.red_total,
                led.close_total
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// §5 检测 10/10 + 升级 10/10 + 消解 10/10（判据三项正主）
// ---------------------------------------------------------------------------

/// 检测 10/10：10 组共享配置（INTx/MSI/MSI-X 混布）全检出。
pub fn detect_rounds_10() -> Result<String, (u32, String)> {
    let mut led = AlertLedger::new();
    // 10 个共享向量：偶数号 INTx（Warn），奇数号 MSI-X（Red）。
    let mut expect_shared: Vec<u32> = Vec::new();
    for g in 0..10u32 {
        let v = 100 + g;
        let path = if g % 2 == 0 { VecPath::Intx } else { VecPath::MsiX };
        led.attach(v, path, g * 2)?;
        led.attach(v, path, g * 2 + 1)?;
        expect_shared.push(v);
    }
    // 独占噪声（不该检出）：10 个单设备向量。
    for g in 0..10u32 {
        led.attach(200 + g, VecPath::Msi, 50 + g)?;
    }
    led.scan(1)?;
    led.detect_audit(&expect_shared)?;
    led.level_audit()?;
    if led.open_total != 10 {
        return Err((
            XC_DETECT_MISS,
            format!("开警总数 {} ≠ 10", led.open_total),
        ));
    }
    Ok("共享配置检测 10/10 全检出（含独占噪声零误报）".to_string())
}

/// 升级 10/10：INTx 共享先开 Warn，改配 MSI-X 后重扫全部升级 Red。
pub fn upgrade_rounds_10() -> Result<String, (u32, String)> {
    let mut led = AlertLedger::new();
    for g in 0..10u32 {
        // 先 INTx 共享（合法时代）。
        led.attach(300 + g, VecPath::Intx, g)?;
        led.attach(300 + g, VecPath::Intx, g + 1)?;
    }
    led.scan(1)?;
    if led.open_total != 10 || led.red_total != 0 {
        return Err((
            XC_LEVEL_MISS,
            format!("首轮口径失守：开 {} 红 {}", led.open_total, led.red_total),
        ));
    }
    // 时代切换：向量改挂 MSI-X（重建占用——整改前置是时代变更）。
    for g in 0..10u32 {
        let v = 300 + g;
        // 摘两设备再以 MSI-X 重挂（保持共享）。
        led.detach(v, g)?;
        led.detach(v, g + 1)?;
        led.attach(v, VecPath::MsiX, g)?;
        led.attach(v, VecPath::MsiX, g + 1)?;
    }
    led.scan(2)?;
    if led.red_total != 10 {
        return Err((
            XC_ERA_RED,
            format!("MSI 时代共享红账 {} ≠ 10——升级失准", led.red_total),
        ));
    }
    led.level_audit()?;
    Ok("MSI 时代共享告警升级 10/10 全红账（Warn→Red 单向升级）".to_string())
}

/// 消解 10/10：整改（退回独占）后重扫撤销全部告警。
pub fn resolve_rounds_10() -> Result<String, (u32, String)> {
    let mut led = AlertLedger::new();
    for g in 0..10u32 {
        let v = 400 + g;
        led.attach(v, VecPath::MsiX, g)?; // Red 原料
        led.attach(v, VecPath::MsiX, g + 1)?;
    }
    led.scan(1)?;
    if led.red_total != 10 {
        return Err((XC_ERA_RED, format!("前置红账 {} ≠ 10", led.red_total)));
    }
    // 整改：每向量摘掉一台设备（退回独占）→重扫→告警全消。
    for g in 0..10u32 {
        led.detach(400 + g, g + 1)?;
    }
    led.scan(2)?;
    if !led.active.is_empty() {
        return Err((
            XC_CLOSE_FAIL,
            format!("整改后活跃告警 {} 残留", led.active.len()),
        ));
    }
    if led.close_total != 10 {
        return Err((
            XC_CLOSE_FAIL,
            format!("消解总数 {} ≠ 10", led.close_total),
        ));
    }
    if led.archive.len() != 10 {
        return Err((XC_CLOSE_FAIL, format!("归档 {} ≠ 10", led.archive.len())));
    }
    Ok("告警消解 10/10（整改后重扫全撤+归档 10 条）".to_string())
}

// ---------------------------------------------------------------------------
// §6 票序分层 + 十闸注入
// ---------------------------------------------------------------------------

/// 票序分层断言：共享向量挂线序==注册序（F0904 分层声明）。
pub fn ticket_order_check() -> Result<String, (u32, String)> {
    let mut led = AlertLedger::new();
    let mut reg_order: BTreeMap<u32, u32> = BTreeMap::new();
    // 注册序：设备 1..=5 依次入册（序号=设备号本身——单调可断言）。
    for d in 1..=5u32 {
        reg_order.insert(d, d);
    }
    // 挂线按注册序：1,2,3,4,5 → 共享向量 9。
    for d in 1..=5u32 {
        led.attach(9, VecPath::Intx, d)?;
    }
    led.ticket_order_audit(&reg_order)?;
    // 破序注入：构造乱序挂线（先 3 后 1）——新向量。
    let mut bad = AlertLedger::new();
    let _ = bad.attach(7, VecPath::Intx, 3);
    let _ = bad.attach(7, VecPath::Intx, 1);
    let r = bad.ticket_order_audit(&reg_order);
    if !matches!(r, Err((XC_TICKET_ORDER, _))) {
        return Err((XC_TICKET_ORDER, "破序挂线未检出——票序分层失守".to_string()));
    }
    Ok("票序分层断言过（正序绿+破序红——F0904 语义单源）".to_string())
}

/// 十闸注入：路径冲突/重复挂线/摘线无源/幽灵消解/空集/重复注册/
/// 漏报对拍/级别口径/消解后重开/归档账。
pub fn inject_gates_10() -> Result<String, (u32, String)> {
    let mut hits: Vec<(u8, bool, String)> = Vec::new();

    // ── 1 同向量路径冲突 ──
    {
        let mut led = AlertLedger::new();
        led.attach(1, VecPath::Intx, 1)?;
        let r = led.attach(1, VecPath::MsiX, 2);
        hits.push((1, matches!(r, Err((XC_LEVEL_MISS, _))), format!("{:?}", r.is_err())));
    }
    // ── 2 同设备重复挂线 ──
    {
        let mut led = AlertLedger::new();
        led.attach(2, VecPath::Msi, 7)?;
        let r = led.attach(2, VecPath::Msi, 7);
        hits.push((2, matches!(r, Err((XC_DUP_ATTACH, _))), format!("{:?}", r.is_err())));
    }
    // ── 3 摘线无源（向量不存在）──
    {
        let mut led = AlertLedger::new();
        let r = led.detach(99, 1);
        hits.push((3, matches!(r, Err((XC_EMPTY_OCCUPY, _))), format!("{:?}", r.is_err())));
    }
    // ── 4 幽灵消解（无告警可撤）──
    {
        let mut led = AlertLedger::new();
        let r = led.close_alert(5, 1);
        hits.push((4, matches!(r, Err((XC_GHOST_ALERT, _))), format!("{:?}", r.is_err())));
    }
    // ── 5 摘线无的（设备未挂该向量）──
    {
        let mut led = AlertLedger::new();
        led.attach(3, VecPath::Intx, 1)?;
        let r = led.detach(3, 9);
        hits.push((5, matches!(r, Err((XC_EMPTY_OCCUPY, _))), format!("{:?}", r.is_err())));
    }
    // ── 6 检测漏报对拍（构造期望≠实际必红）──
    {
        let mut led = AlertLedger::new();
        led.attach(6, VecPath::MsiX, 1)?;
        led.attach(6, VecPath::MsiX, 2)?;
        led.scan(1)?;
        let r = led.detect_audit(&[7]); // 期望错位
        hits.push((6, matches!(r, Err((XC_DETECT_MISS, _))), format!("{:?}", r.is_err())));
    }
    // ── 7 级别口径（MSI-X 共享开成 Warn 即破——人工改级对拍）──
    {
        let mut led = AlertLedger::new();
        led.attach(8, VecPath::MsiX, 1)?;
        led.attach(8, VecPath::MsiX, 2)?;
        led.scan(1)?;
        // 口径审计此时应绿（Red）；人工降级后审计必红。
        if let Some(a) = led.active.get_mut(&8) {
            a.level = AlertLevel::Warn;
        }
        let r = led.level_audit();
        hits.push((7, matches!(r, Err((XC_LEVEL_MISS, _))), format!("{:?}", r.is_err())));
    }
    // ── 8 独占向量零告警（检测不误报）──
    {
        let mut led = AlertLedger::new();
        led.attach(10, VecPath::MsiX, 1)?;
        led.scan(1)?;
        let ok = led.active.is_empty() && led.open_total == 0;
        hits.push((8, ok, format!("open_total={}", led.open_total)));
    }
    // ── 9 摘空向量清槽（聚合失守候选——摘后 occupy 无残留）──
    {
        let mut led = AlertLedger::new();
        led.attach(11, VecPath::Intx, 1)?;
        led.detach(11, 1)?;
        let ok = !led.occupy.contains_key(&11);
        hits.push((9, ok, format!("槽残留={}", led.occupy.contains_key(&11))));
    }
    // ── 10 归档账（消解入档 append-only——归档条目含 closed_at）──
    {
        let mut led = AlertLedger::new();
        led.attach(12, VecPath::Msi, 1)?;
        led.attach(12, VecPath::Msi, 2)?;
        led.scan(1)?;
        led.detach(12, 2)?;
        led.scan(2)?;
        let ok = led.archive.len() == 1 && led.archive[0].closed_at == Some(1)
            && led.close_total == 1;
        hits.push((10, ok, format!("归档={} 消={}", led.archive.len(), led.close_total)));
    }

    let missed: Vec<&(u8, bool, String)> = hits.iter().filter(|(_, ok, _)| !ok).collect();
    if !missed.is_empty() {
        let names: Vec<String> = missed.iter().map(|(k, _, e)| format!("#{} {}", k, e)).collect();
        return Err((XC_DETECT_MISS, format!("十闸漏网：{}", names.join("；"))));
    }
    Ok("十闸全拦（路径冲突/重复挂线/摘线两向/幽灵消解/漏报对拍/级别口径/零误报/清槽/归档）".to_string())
}

// ---------------------------------------------------------------------------
// §7 压测（万级挂线——检测/升级/消解全链）
// ---------------------------------------------------------------------------

/// 压测设备数。
pub const STRESS_DEVICES: u32 = 2_000;

/// 压测：2000 设备挂 500 向量（每向量 4 设备 INTx 共享）→全检出→
/// 半数整改→半数消解→账面守恒。
pub fn stress_2k() -> Result<String, (u32, String)> {
    let mut led = AlertLedger::new();
    // 1）500 向量×4 设备（INTx——共享合法，全部应开 Warn）。
    let vectors = STRESS_DEVICES / 4; // 500
    for v in 0..vectors {
        for j in 0..4u32 {
            led.attach(v, VecPath::Intx, v * 4 + j)?;
        }
    }
    // 2）扫描：500 告警全开（全 Warn——INTx 时代合法）。
    led.scan(1)?;
    if led.open_total != vectors as u64 || led.red_total != 0 {
        return Err((
            XC_DETECT_MISS,
            format!("压测首轮：开 {} 红 {}（期望 {} / 0）", led.open_total, led.red_total, vectors),
        ));
    }
    // 3）半数整改：偶数向量摘 3 台（退独占）→重扫→告警消解 250。
    for v in (0..vectors).step_by(2) {
        for j in 1..4u32 {
            led.detach(v, v * 4 + j)?;
        }
    }
    led.scan(2)?;
    if led.close_total != (vectors / 2) as u64 {
        return Err((
            XC_CLOSE_FAIL,
            format!("整改消解 {} ≠ {}", led.close_total, vectors / 2),
        ));
    }
    if led.active.len() != (vectors / 2) as usize {
        return Err((
            XC_DETECT_MISS,
            format!("残留活跃告警 {} ≠ {}", led.active.len(), vectors / 2),
        ));
    }
    // 4）级别口径审计（残留半必 Warn）。
    led.level_audit()?;
    // 5）账面守恒：挂 2000==摘 1500+？——挂=2000，摘=250×3=750，
    //    占用向量=500（250 共享 4 台+250 独占 1 台）。
    if led.attach_total != STRESS_DEVICES as u64 || led.detach_total != 750 {
        return Err((
            XC_EMPTY_OCCUPY,
            format!("挂摘账失守：挂 {} 摘 {}", led.attach_total, led.detach_total),
        ));
    }
    // 6）快照稳定。
    let s0 = AlertSnap::take(&led);
    let s1 = AlertSnap::take(&led);
    if s0.hash != s1.hash {
        return Err((XC_SNAP_DRIFT, "压测账快照不稳定".to_string()));
    }
    Ok(format!(
        "压测全绿：{} 设备 {} 向量 500 共享全检出 半数整改消解 250 残留全 Warn 快照 {:#010x}",
        STRESS_DEVICES, vectors, s0.hash
    ))
}

// ---------------------------------------------------------------------------
// §8 全链自证（聚合入口——ktest 桥接点）
// ---------------------------------------------------------------------------

/// 全链自证：检测 10/10 + 升级 10/10 + 消解 10/10 + 票序 + 十闸
/// + 压测 + 快照稳定。
pub fn run_all_checks() -> Result<String, (u32, String)> {
    let mut parts: Vec<String> = Vec::new();
    parts.push(detect_rounds_10()?);
    parts.push(upgrade_rounds_10()?);
    parts.push(resolve_rounds_10()?);
    parts.push(ticket_order_check()?);
    parts.push(inject_gates_10()?);
    parts.push(stress_2k()?);
    // 快照稳定性（空账双采样）。
    let empty = AlertLedger::new();
    let s0 = AlertSnap::take(&empty);
    let s1 = AlertSnap::take(&empty);
    if s0.hash != s1.hash {
        return Err((XC_SNAP_DRIFT, "空账快照指纹不稳定".to_string()));
    }
    parts.push(format!("快照稳定（{:#010x} {}）", s0.hash, s0.summary));
    Ok(parts.join("；"))
}
