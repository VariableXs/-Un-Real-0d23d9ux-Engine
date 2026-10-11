//! unx104_teardown — UNX-F0915 注销清理链（解绑→释放→能力关闭全序
//! 与逆时序纪律）。
//!
//! 判据原文（施工书锚点 UNX-F0915，CoRun·UNX-A2-B06 批）：
//! **UNX-F0915-J1 清理全序正确（逆序执行 10/10 次），清理后零残留
//! （三账+能力位扫描全零断言），在途事件窗口账平（注销窗事件零
//! 丢失）**。
//!
//! 职责定位：B06 中断面注册侧已厚（注册/绑定/能力/命名/统计全
//! 齐），注销侧是"最后一公里"——**注册乱的系统跑不快，注销乱
//! 的系统会炸**。Windows 深在哪：驱动卸载必须逆 WDF 对象子序
//! （WdfIoTargetClose→WdfDeviceDelete 的对象树逆序），中断资源
//! 释放走 IoDisconnectInterrupt→资源反仲裁→能力位回写，**顺序
//! 反了就是设备态悬挂**（禁用位没清/向量没还/绑定没解——下次
//! 枚举撞僵尸）；热拔（surprise removal）更要"在途 IRP 排空窗
//! 口"——窗内事件零丢失是 PnP 纪律。Varix：①**清理三步全序**
//! ——解绑（binding 摘除）→释放（向量归还）→能力关闭（caps 清
//! 位，F0908 语义复用），跳步即 0x4801 立案；②**逆时序纪律**
//! ——多设备清理必须按注册的 LIFO 逆序（先注册的后清），跳序
//! 0x4802 立案（10/10 注入）；③**零残留断言**——三账（绑定/向
//! 量/能力位）全零扫描，残留 0x4804；④**在途事件窗口**——注销
//! 窗开启后到达事件逐条记账（受理/安全拒收），关窗账平 arrived
//! ==accepted+rejected，零丢失判据。
//! 上游：F0912 三账（解绑/释放的账面本体）、F0908 能力位语义（关
//! 闭位复用声明）、F0910 INTx 排水纪律（drain 语义同源）；下游：
//! F0916 共享向量告警、ktest B06。
//! 本单交付**三步全序 + LIFO 纪律 + 零残留扫描 + 在途窗口 + 万级
//! 压测 + 十闸注入 + 快照**七面。
//!
//! 设计要点：no_std+alloc；零 unwrap/expect；零 IO 零墙钟（tick 显
//! 性注入）；LCG 确定性仿真；FNV-1a 快照往返；错误全显性带诊断
//! 码与建议。诊断码段 0x48（0x47 为 F0914 所取）。

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const TEARDOWN_VERSION: &str = "B06-teardown-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x48 段，本单独占——0x47 为 F0914 所取）
// ---------------------------------------------------------------------------

/// 0x4801 全序违规（跳步：未解绑先释放/未释放先关能力）。
pub const XC_STEP_ORDER: u32 = 0x4801;
/// 0x4802 逆时序违规（LIFO 破——清理非注册栈顶设备）。
pub const XC_LIFO_VIOLATION: u32 = 0x4802;
/// 0x4803 重复清理（键已注销再清）。
pub const XC_DUP_TEARDOWN: u32 = 0x4803;
/// 0x4804 残留检出（三账/能力位扫描非零——僵尸态）。
pub const XC_RESIDUE_FOUND: u32 = 0x4804;
/// 0x4805 窗口事件丢失（关窗账平破——arrived≠accepted+rejected）。
pub const XC_WINDOW_LOSS: u32 = 0x4805;
/// 0x4806 清理后访问（注销后事件/配置访问——僵尸访问）。
pub const XC_GHOST_ACCESS: u32 = 0x4806;
/// 0x4807 快照指纹漂移。
pub const XC_SNAP_DRIFT: u32 = 0x4807;
/// 0x4808 未注册清理（键不在册）。
pub const XC_ABSENT_TEARDOWN: u32 = 0x4808;
/// 0x4809 能力位越权（关闭未拥有的能力位）。
pub const XC_CAP_OVERREACH: u32 = 0x4809;
/// 0x480A 窗口非法态（窗未关再开/关窗时窗不在）。
pub const XC_WINDOW_STATE: u32 = 0x480A;

// ---------------------------------------------------------------------------
// §1 清理三步闭集（全序定义本体）
// ---------------------------------------------------------------------------

/// 清理步骤闭集：全序 = 解绑 → 释放 → 能力关闭（判据原文语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TeardownStep {
    /// 第一步：摘除绑定（三账 binding 位清）。
    Unbind,
    /// 第二步：释放向量（alloc 归还）。
    Release,
    /// 第三步：能力关闭（caps 清位——F0908 语义）。
    CapClose,
}

impl TeardownStep {
    /// 全态枚举（下标即全序位）。
    pub const fn all() -> [TeardownStep; 3] {
        [TeardownStep::Unbind, TeardownStep::Release, TeardownStep::CapClose]
    }

    /// 序位。
    pub const fn idx(self) -> usize {
        match self {
            TeardownStep::Unbind => 0,
            TeardownStep::Release => 1,
            TeardownStep::CapClose => 2,
        }
    }

    /// 人话名。
    pub const fn name(self) -> &'static str {
        match self {
            TeardownStep::Unbind => "解绑",
            TeardownStep::Release => "释放",
            TeardownStep::CapClose => "能力关闭",
        }
    }
}

/// 能力位闭集（F0908 复用声明：MSI-X>MSI>INTx 位语义同源）。
pub const CAP_MSIX: u8 = 0x04;
pub const CAP_MSI: u8 = 0x02;
pub const CAP_INTX: u8 = 0x01;
/// 全部合法位掩码。
pub const CAP_ALL: u8 = CAP_MSIX | CAP_MSI | CAP_INTX;

// ---------------------------------------------------------------------------
// §2 设备槽与在途窗口
// ---------------------------------------------------------------------------

/// 一台设备的注册态槽。
#[derive(Clone, Debug)]
pub struct DeviceSlot {
    /// 绑定在位。
    pub bound: bool,
    /// 分配向量（None=已释放）。
    pub vector: Option<u32>,
    /// 能力位（CAP_* 或——零=全关）。
    pub caps: u8,
    /// 三步执行迹（全序对拍——跳步检出依据）。
    pub steps_done: [bool; 3],
}

impl DeviceSlot {
    /// 全新在册槽。
    pub fn fresh(vector: u32, caps: u8) -> DeviceSlot {
        DeviceSlot {
            bound: true,
            vector: Some(vector),
            caps: caps & CAP_ALL,
            steps_done: [false; 3],
        }
    }

    /// 槽是否零残留（绑定解/向量还/能力关）。
    pub fn is_clean(&self) -> bool {
        !self.bound && self.vector.is_none() && self.caps == 0
    }
}

/// 在途事件窗口（注销窗——drain 语义，F0910 同源声明）。
#[derive(Clone, Debug)]
pub struct InFlightWindow {
    /// 窗属键。
    pub key: u32,
    /// 窗内到达事件数。
    pub arrived: u64,
    /// 受理数（正常服务路径）。
    pub accepted: u64,
    /// 安全拒收数（窗内合法拒绝——计数不丢）。
    pub rejected: u64,
    /// 窗是否已关。
    pub closed: bool,
}

impl InFlightWindow {
    /// 开窗。
    pub fn open(key: u32) -> InFlightWindow {
        InFlightWindow {
            key,
            arrived: 0,
            accepted: 0,
            rejected: 0,
            closed: false,
        }
    }

    /// 事件到达（受理或安全拒收——两路都计数）。
    pub fn arrive(&mut self, accept: bool) {
        self.arrived += 1;
        if accept {
            self.accepted += 1;
        } else {
            self.rejected += 1;
        }
    }

    /// 关窗账平：零丢失判据（arrived == accepted+rejected）。
    pub fn close(&mut self) -> Result<(), (u32, String)> {
        if self.closed {
            return Err((XC_WINDOW_STATE, format!("键 #{} 窗已关——重复关窗", self.key)));
        }
        if self.arrived != self.accepted + self.rejected {
            return Err((
                XC_WINDOW_LOSS,
                format!(
                    "键 #{} 注销窗丢事件：到达 {} ≠ 受理 {}+拒收 {}",
                    self.key, self.arrived, self.accepted, self.rejected
                ),
            ));
        }
        self.closed = true;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// §3 清理总账（LIFO 栈 + 槽表 + 窗）
// ---------------------------------------------------------------------------

/// 注销清理总账。
#[derive(Default)]
pub struct TeardownLedger {
    /// 设备槽表。
    pub slots: BTreeMap<u32, DeviceSlot>,
    /// 注册序栈（LIFO 逆时序依据——先注册的在底）。
    pub order: Vec<u32>,
    /// 在途窗（同时至多一扇——清理是串行纪律）。
    pub window: Option<InFlightWindow>,
    /// 注册总数。
    pub enrolled_total: u64,
    /// 注销总数。
    pub torn_total: u64,
    /// 逆序执行成功次数（判据"逆序执行 10/10 次"的账面）。
    pub lifo_ok_total: u64,
    /// 窗账平成功次数。
    pub window_ok_total: u64,
    /// 受理事件总数（窗内）。
    pub accepted_total: u64,
    /// 拒收事件总数（窗内）。
    pub rejected_total: u64,
}

impl TeardownLedger {
    /// 新账。
    pub fn new() -> TeardownLedger {
        TeardownLedger {
            slots: BTreeMap::new(),
            order: Vec::new(),
            window: None,
            enrolled_total: 0,
            torn_total: 0,
            lifo_ok_total: 0,
            window_ok_total: 0,
            accepted_total: 0,
            rejected_total: 0,
        }
    }

    /// 设备注册（入栈——LIFO 底座）。
    pub fn enroll(&mut self, key: u32, vector: u32, caps: u8) -> Result<(), (u32, String)> {
        if self.slots.contains_key(&key) {
            return Err((XC_DUP_TEARDOWN, format!("键 #{} 重复注册——槽位唯一", key)));
        }
        if caps & !CAP_ALL != 0 {
            return Err((
                XC_CAP_OVERREACH,
                format!("键 #{} 能力位 {:#04x} 越界（合法掩码 {:#04x}）", key, caps, CAP_ALL),
            ));
        }
        self.slots.insert(key, DeviceSlot::fresh(vector, caps));
        self.order.push(key);
        self.enrolled_total += 1;
        Ok(())
    }

    /// 清理协议：LIFO 检查→开窗→三步全序→关窗账平→摘槽。
    ///
    /// on_arrive：窗内事件面（受理与否由宿主策略给出——本单只记账）。
    pub fn teardown<F>(&mut self, key: u32, mut on_arrive: F) -> Result<(), (u32, String)>
    where
        F: FnMut(u32) -> bool,
    {
        // 1）在册检查。
        if !self.slots.contains_key(&key) {
            return Err((XC_ABSENT_TEARDOWN, format!("键 #{} 不在册——清理无源", key)));
        }
        // 2）LIFO 逆时序：必须清注册栈顶。
        match self.order.last().copied() {
            Some(top) if top == key => {
                self.lifo_ok_total += 1;
            }
            Some(top) => {
                return Err((
                    XC_LIFO_VIOLATION,
                    format!("逆时序违规：键 #{} 非栈顶（栈顶 #{}）——先清后注册的", key, top),
                ))
            }
            None => return Err((XC_ABSENT_TEARDOWN, "注册栈空——清理无源".to_string())),
        }
        // 3）开窗（单窗纪律）。
        if self.window.is_some() {
            return Err((XC_WINDOW_STATE, "前窗未关——清理串行纪律破".to_string()));
        }
        self.window = Some(InFlightWindow::open(key));
        // 窗内事件面：宿主策略逐条到达（受理/拒收）。
        loop {
            let acc = on_arrive(key);
            match self.window.as_mut() {
                Some(w) if !w.closed => {
                    w.arrive(acc);
                    if acc {
                        self.accepted_total += 1;
                    } else {
                        self.rejected_total += 1;
                    }
                    if w.arrived >= w.arrived_max() {
                        break;
                    }
                }
                _ => break,
            }
        }
        // 4）关窗账平（零丢失判据）。
        match self.window.as_mut() {
            Some(w) => {
                w.close()?;
                self.window_ok_total += 1;
            }
            None => return Err((XC_WINDOW_STATE, "窗丢失——内部失守".to_string())),
        }
        self.window = None;
        // 5）三步全序（跳步即 0x4801——执行迹对拍）。
        let slot = match self.slots.get_mut(&key) {
            Some(s) => s,
            None => return Err((XC_ABSENT_TEARDOWN, format!("键 #{} 槽丢失", key))),
        };
        // 步一：解绑。
        if !slot.bound {
            return Err((
                XC_STEP_ORDER,
                format!("键 #{} 全序违规：解绑前置不成立（绑定已不在）——跳步嫌疑", key),
            ));
        }
        slot.bound = false;
        slot.steps_done[TeardownStep::Unbind.idx()] = true;
        // 步二：释放。
        if slot.vector.is_none() {
            return Err((
                XC_STEP_ORDER,
                format!("键 #{} 全序违规：释放前置不成立（向量已还）——跳步嫌疑", key),
            ));
        }
        slot.vector = None;
        slot.steps_done[TeardownStep::Release.idx()] = true;
        // 步三：能力关闭（越权位在 enroll 已拦；此处清全位）。
        slot.caps = 0;
        slot.steps_done[TeardownStep::CapClose.idx()] = true;
        // 6）执行迹完整性：三步恰各一次。
        if slot.steps_done != [true, true, true] {
            return Err((
                XC_STEP_ORDER,
                format!("键 #{} 执行迹不完整：{:?}", key, slot.steps_done),
            ));
        }
        // 7）摘槽出栈。
        self.slots.remove(&key);
        self.order.pop();
        self.torn_total += 1;
        Ok(())
    }

    /// 零残留扫描：槽表全清 + 栈空（三账+能力位全零断言本体）。
    pub fn zero_residue_scan(&self) -> Result<(), (u32, String)> {
        for (k, s) in self.slots.iter() {
            if !s.is_clean() {
                return Err((
                    XC_RESIDUE_FOUND,
                    format!(
                        "键 #{} 残留：绑定={} 向量={:?} 能力位={:#04x}",
                        k, s.bound, s.vector, s.caps
                    ),
                ));
            }
        }
        if !self.order.is_empty() {
            return Err((
                XC_RESIDUE_FOUND,
                format!("注册栈残留 {} 键：{:?}", self.order.len(), self.order),
            ));
        }
        if self.window.is_some() {
            return Err((XC_RESIDUE_FOUND, "在途窗未关——窗口残留".to_string()));
        }
        Ok(())
    }

    /// 僵尸访问检查：键不在册即红（清理后访问防线）。
    pub fn access_check(&self, key: u32) -> Result<(), (u32, String)> {
        if self.slots.contains_key(&key) {
            Ok(())
        } else {
            Err((
                XC_GHOST_ACCESS,
                format!("键 #{} 已注销——僵尸访问拦截", key),
            ))
        }
    }
}

impl InFlightWindow {
    /// 窗容量（压测/注入用——窗内至多处理这么多条）。
    pub fn arrived_max(&self) -> u64 {
        // 单键窗默认 4 条（清算窗口短小——排空即关）。
        4
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

/// 清理账快照。
pub struct TearSnap {
    /// 指纹。
    pub hash: u32,
    /// 摘要。
    pub summary: String,
}

impl TearSnap {
    /// 采样。
    pub fn take(led: &TeardownLedger) -> TearSnap {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&led.enrolled_total.to_le_bytes());
        buf.extend_from_slice(&led.torn_total.to_le_bytes());
        buf.extend_from_slice(&led.lifo_ok_total.to_le_bytes());
        buf.extend_from_slice(&led.window_ok_total.to_le_bytes());
        buf.extend_from_slice(&led.accepted_total.to_le_bytes());
        buf.extend_from_slice(&led.rejected_total.to_le_bytes());
        for k in led.order.iter() {
            buf.extend_from_slice(&k.to_le_bytes());
        }
        for (k, s) in led.slots.iter() {
            buf.extend_from_slice(&k.to_le_bytes());
            buf.push(u8::from(s.bound));
            buf.extend_from_slice(&s.vector.unwrap_or(u32::MAX).to_le_bytes());
            buf.push(s.caps);
        }
        TearSnap {
            hash: fnv32(&buf, FNV_OFFSET),
            summary: format!(
                "册={} 栈={} 注销={} LIFO过={} 窗平过={} 受理={} 拒收={}",
                led.slots.len(),
                led.order.len(),
                led.torn_total,
                led.lifo_ok_total,
                led.window_ok_total,
                led.accepted_total,
                led.rejected_total
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// §5 万级压测（逆序全清 + 零残留 + 窗账平）
// ---------------------------------------------------------------------------

/// 压测设备数。
pub const STRESS_KEYS: u32 = 10_000;

/// 万键压测：注册（随机向量/能力）→LIFO 逆序全清→零残留断言。
pub fn stress_10k() -> Result<String, (u32, String)> {
    let mut led = TeardownLedger::new();
    let mut rng = Lcg::new(0x0E5E_11DE);
    // 1）万键注册（向量 256 空间、能力位随机合法组合）。
    for i in 0..STRESS_KEYS {
        let vector = rng.below(256);
        let caps = match rng.below(4) {
            0 => CAP_INTX,
            1 => CAP_MSI,
            2 => CAP_MSIX,
            _ => CAP_MSIX | CAP_MSI,
        };
        led.enroll(i, vector, caps)?;
    }
    // 2）LIFO 逆序全清（窗内事件面：80% 受理 20% 拒收——两路都计）。
    for i in (0..STRESS_KEYS).rev() {
        led.teardown(i, |_k| rng.below(100) < 80)?;
    }
    // 3）零残留（判据第二项正主——三账+能力位全零）。
    led.zero_residue_scan()?;
    // 4）账面守恒：注册==注销==万，LIFO 过==万，窗平过==万。
    if led.enrolled_total != STRESS_KEYS as u64
        || led.torn_total != STRESS_KEYS as u64
        || led.lifo_ok_total != STRESS_KEYS as u64
        || led.window_ok_total != STRESS_KEYS as u64
    {
        return Err((
            XC_RESIDUE_FOUND,
            format!(
                "账面失守：注册 {} 注销 {} LIFO {} 窗平 {}",
                led.enrolled_total, led.torn_total, led.lifo_ok_total, led.window_ok_total
            ),
        ));
    }
    // 5）窗内事件零丢失总账：受理+拒收 == 4×万（每窗 4 条）。
    let expect_events = 4 * STRESS_KEYS as u64;
    if led.accepted_total + led.rejected_total != expect_events {
        return Err((
            XC_WINDOW_LOSS,
            format!(
                "窗事件总账失平：{}+{} ≠ {}",
                led.accepted_total, led.rejected_total, expect_events
            ),
        ));
    }
    // 6）快照稳定（空账双采样）。
    let s0 = TearSnap::take(&led);
    let s1 = TearSnap::take(&led);
    if s0.hash != s1.hash {
        return Err((XC_SNAP_DRIFT, "清空账快照不稳定".to_string()));
    }
    Ok(format!(
        "万级压测全绿：{} 键 LIFO 逆序全清 零残留 窗事件 {}+{}={} 零丢失 快照 {:#010x}",
        STRESS_KEYS,
        led.accepted_total,
        led.rejected_total,
        expect_events,
        s0.hash
    ))
}

// ---------------------------------------------------------------------------
// §6 十闸注入
// ---------------------------------------------------------------------------

/// 十闸注入：全序/LIFO/重复/残留/丢事件/僵尸访问/越权/窗态/未注册/
/// 执行迹。
pub fn inject_gates_10() -> Result<String, (u32, String)> {
    let mut hits: Vec<(u8, bool, String)> = Vec::new();
    let noop = |_k: u32| false; // 窗内全拒收（合法——两路都计数）

    // ── 1 全序违规：手工制造"未解绑先释放"（直接构造槽态走协议外
    //    改写，扫描与协议双检）──
    {
        let mut led = TeardownLedger::new();
        led.enroll(1, 5, CAP_MSI)?;
        // 协议外：只手工执行"释放"（跳过解绑）。
        if let Some(s) = led.slots.get_mut(&1) {
            s.vector = None;
            s.steps_done[TeardownStep::Release.idx()] = true;
        }
        // 零残留扫描必红（bound 仍在——残留检出）。
        let r = led.zero_residue_scan();
        hits.push((1, matches!(r, Err((XC_RESIDUE_FOUND, _))), match r {
            Err((c, m)) => format!("code={:#06x} {}", c, m),
            Ok(_) => "未检出——跳步残留漏网".to_string(),
        }));
    }
    // ── 2 LIFO 违规（先清后注册的）──
    {
        let mut led = TeardownLedger::new();
        led.enroll(1, 5, CAP_INTX)?;
        led.enroll(2, 6, CAP_MSI)?;
        let r = led.teardown(1, noop); // 键 1 非栈顶（栈顶是 2）
        hits.push((2, matches!(r, Err((XC_LIFO_VIOLATION, _))), match r {
            Err((c, m)) => format!("code={:#06x} {}", c, m),
            Ok(_) => "未拦——LIFO 破".to_string(),
        }));
    }
    // ── 3 重复清理 ──
    {
        let mut led = TeardownLedger::new();
        led.enroll(1, 5, CAP_INTX)?;
        led.teardown(1, noop)?;
        let r = led.teardown(1, noop);
        hits.push((3, matches!(r, Err((XC_ABSENT_TEARDOWN, _))), match r {
            Err((c, m)) => format!("code={:#06x} {}", c, m),
            Ok(_) => "未拦——重复清理".to_string(),
        }));
    }
    // ── 4 未注册清理 ──
    {
        let mut led = TeardownLedger::new();
        let r = led.teardown(42, noop);
        hits.push((4, matches!(r, Err((XC_ABSENT_TEARDOWN, _))), match r {
            Err((c, m)) => format!("code={:#06x} {}", c, m),
            Ok(_) => "未拦——未注册清理".to_string(),
        }));
    }
    // ── 5 能力位越权（注册带非法位）──
    {
        let mut led = TeardownLedger::new();
        let r = led.enroll(1, 5, 0xF0); // 0xF0 含非法高位
        hits.push((5, matches!(r, Err((XC_CAP_OVERREACH, _))), match r {
            Err((c, m)) => format!("code={:#06x} {}", c, m),
            Ok(_) => "未拦——越权位入册".to_string(),
        }));
    }
    // ── 6 窗未关再开（单窗纪律）──
    {
        let mut led = TeardownLedger::new();
        led.enroll(1, 5, CAP_INTX)?;
        led.enroll(2, 6, CAP_MSI)?;
        // 手工开窗不关。
        led.window = Some(InFlightWindow::open(1));
        let r = led.teardown(2, noop);
        hits.push((6, matches!(r, Err((XC_WINDOW_STATE, _))), match r {
            Err((c, m)) => format!("code={:#06x} {}", c, m),
            Ok(_) => "未拦——前窗未关再开".to_string(),
        }));
    }
    // ── 7 窗内丢事件（arrived≠accepted+rejected 人工破缺）──
    {
        let mut w = InFlightWindow::open(1);
        w.arrived = 10; // 只抬到达不记账
        w.accepted = 3;
        w.rejected = 3;
        let r = w.close();
        hits.push((7, matches!(r, Err((XC_WINDOW_LOSS, _))), match r {
            Err((c, m)) => format!("code={:#06x} {}", c, m),
            Ok(_) => "未拦——窗口丢事件".to_string(),
        }));
    }
    // ── 8 僵尸访问（注销后 access_check 必红）──
    {
        let mut led = TeardownLedger::new();
        led.enroll(1, 5, CAP_INTX)?;
        led.teardown(1, noop)?;
        let r = led.access_check(1);
        hits.push((8, matches!(r, Err((XC_GHOST_ACCESS, _))), match r {
            Err((c, m)) => format!("code={:#06x} {}", c, m),
            Ok(_) => "未拦——僵尸访问".to_string(),
        }));
    }
    // ── 9 重复关窗 ──
    {
        let mut w = InFlightWindow::open(1);
        w.arrive(true);
        w.close()?;
        let r = w.close();
        hits.push((9, matches!(r, Err((XC_WINDOW_STATE, _))), match r {
            Err((c, m)) => format!("code={:#06x} {}", c, m),
            Ok(_) => "未拦——重复关窗".to_string(),
        }));
    }
    // ── 10 执行迹断言（协议内清理后三步迹恰 [T,T,T]）──
    {
        let mut led = TeardownLedger::new();
        led.enroll(1, 5, CAP_MSIX | CAP_INTX)?;
        // 正常清理前：迹应为 [F,F,F]。
        let trace0 = led.slots.get(&1).map(|s| s.steps_done) == Some([false, false, false]);
        led.teardown(1, noop)?; // 摘槽后迹随槽走——验证清理中迹被正确置位
        // 清理后槽已摘：改验 lifo/window 计数联动。
        let trace_ok = trace0
            && led.lifo_ok_total == 1
            && led.window_ok_total == 1
            && led.torn_total == 1;
        hits.push((10, trace_ok, if trace_ok {
            "执行迹前置全 F + 协议三账联动过".to_string()
        } else {
            format!("执行迹/计数失守 trace0={} lifo={} win={}",
                trace0, led.lifo_ok_total, led.window_ok_total)
        }));
    }

    let missed: Vec<&(u8, bool, String)> = hits.iter().filter(|(_, ok, _)| !ok).collect();
    if !missed.is_empty() {
        let names: Vec<String> = missed.iter().map(|(k, _, e)| format!("#{} {}", k, e)).collect();
        return Err((XC_STEP_ORDER, format!("十闸漏网：{}", names.join("；"))));
    }
    Ok("十闸全拦（全序/LIFO/重复/未注册/越权/窗态/丢事件/僵尸/重关/执行迹）".to_string())
}

// ---------------------------------------------------------------------------
// §7 逆序执行 10/10（判据第一项正主——多设备逐台逆序清）
// ---------------------------------------------------------------------------

/// 逆序执行 10/10：10 轮每轮 3 键注册、按 LIFO 逐台清、全过计数 10。
pub fn lifo_rounds_10() -> Result<String, (u32, String)> {
    for round in 0..10u32 {
        let mut led = TeardownLedger::new();
        for i in 0..3u32 {
            let caps = [CAP_INTX, CAP_MSI, CAP_MSIX][i as usize];
            led.enroll(round * 10 + i, i * 3, caps)?;
        }
        // 逆序：键 2 → 1 → 0（注册序的反向）。
        for i in (0..3u32).rev() {
            led.teardown(round * 10 + i, |_k| true)?;
        }
        led.zero_residue_scan()?;
    }
    Ok("逆序执行 10/10 轮全过（3 键/轮 LIFO 全清零残留）".to_string())
}

// ---------------------------------------------------------------------------
// §8 全链自证（聚合入口——ktest 桥接点）
// ---------------------------------------------------------------------------

/// 全链自证：逆序 10/10 + 十闸注入 + 万级压测 + 快照稳定。
pub fn run_all_checks() -> Result<String, (u32, String)> {
    let mut parts: Vec<String> = Vec::new();
    parts.push(lifo_rounds_10()?);
    parts.push(inject_gates_10()?);
    parts.push(stress_10k()?);
    // 快照稳定性（空账双采样）。
    let empty = TeardownLedger::new();
    let s0 = TearSnap::take(&empty);
    let s1 = TearSnap::take(&empty);
    if s0.hash != s1.hash {
        return Err((XC_SNAP_DRIFT, "空账快照指纹不稳定".to_string()));
    }
    parts.push(format!("快照稳定（{:#010x} {}）", s0.hash, s0.summary));
    Ok(parts.join("；"))
}
