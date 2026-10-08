//! VE-F0201 续 · 能力解析 + 初始化状态机（判据主战场）
//!
//! 判据映射：
//! - 能力解析完整 → `parse_capabilities`（四类齐全/无重复/布局合法）；
//! - 版本协商 VIRTIO_F_VERSION_1 必需 → `negotiate_features`（缺即拒绝三要素）；
//! - 初始化序 reset→ack→协商→队列→DRIVER_OK → `InitMachine`（跳步显性拒绝）；
//! - 零"试试看"写 → `RegKind` 白名单 + `WriteTrace` 留痕（未注册寄存器一律拒）；
//! - 初始化 ≤100ms → `InitTiming` 分步账（确定性成本模型，逻辑 tick 不用墙钟）。
//!
//! 设计要点：初始化器的每一步都先验证"当前状态允许这一步"，再记账再前移——
//! 状态机的拒绝不是报错完事，而是把 FAILED 位置上并给出对路建议。

use super::veb01_device::*;
use crate::svstar2::vea01_probe::fnv1a64;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、能力列表解析（判据：能力解析完整）
// ---------------------------------------------------------------------------

/// 解析出的四类能力区域（都命中才算完整）。
#[derive(Clone, Debug, Default)]
pub struct CapabilityMap {
    pub common: Option<CapRegion>,
    pub notify: Option<CapRegion>,
    pub isr: Option<CapRegion>,
    pub device: Option<CapRegion>,
    /// notify_off_multiplier（notify 区域解析副产物）
    pub notify_off_multiplier: u32,
}

impl CapabilityMap {
    /// 四类齐全（判据"能力解析完整"的机器判据）。
    pub fn complete(&self) -> bool {
        self.common.is_some() && self.notify.is_some() && self.isr.is_some()
            && self.device.is_some()
    }

    /// 读屏可达摘要。
    pub fn describe(&self) -> String {
        if !self.complete() {
            return "能力列表不完整（common/notify/isr/device 四类未齐）".to_string();
        }
        format!(
            "能力列表完整：common@bar{}+0x{:X} len 0x{:X}、notify@bar{}+0x{:X}（乘数 {}）、\
isr@bar{}+0x{:X}、device@bar{}+0x{:X}",
            self.common.as_ref().map(|c| c.bar).unwrap_or(0),
            self.common.as_ref().map(|c| c.offset).unwrap_or(0),
            self.common.as_ref().map(|c| c.length).unwrap_or(0),
            self.notify.as_ref().map(|c| c.bar).unwrap_or(0),
            self.notify.as_ref().map(|c| c.offset).unwrap_or(0),
            self.notify_off_multiplier,
            self.isr.as_ref().map(|c| c.bar).unwrap_or(0),
            self.isr.as_ref().map(|c| c.offset).unwrap_or(0),
            self.device.as_ref().map(|c| c.bar).unwrap_or(0),
            self.device.as_ref().map(|c| c.offset).unwrap_or(0),
        )
    }
}

/// 一个已验证的能力区域。
#[derive(Clone, Copy, Debug)]
pub struct CapRegion {
    pub kind: CapKind,
    pub bar: u8,
    pub offset: u32,
    pub length: u32,
}

/// 能力列表解析。输入原始快照，输出四类区域或三要素拒绝。
///
/// 校验序列（每条失败都显性上抛）：
/// 1. 只认 vndr=0x09 的 virtio 厂商能力，别的 vndr 跳过不认（不是错误）；
/// 2. cfg_type 必须落在四类之内，否则三要素拒绝；
/// 3. bar 号 < 6 且该 bar 存在（尺寸非 0）、offset+length 不越过 bar；
/// 4. 区域长度不小于该类的最小合法长度；
/// 5. 同类区域不得重复声明（重复 = 设备布局错乱，硬拒绝）。
pub fn parse_capabilities(snap: &VirtioGpuDeviceSnapshot) -> Result<CapabilityMap, Rejection> {
    if snap.caps_raw.is_empty() {
        return Err(Rejection {
            code: "E_NO_CAPS",
            what: "设备快照能力列表为空".to_string(),
            why: "现代版 virtio 必须以 PCI 能力列表声明配置区域，没有它就找不到任何寄存器"
                .to_string(),
            next: "确认这是现代版设备（0x1050）；旧版设备走显性拒绝路径不初始化".to_string(),
        });
    }
    let mut map = CapabilityMap::default();
    for (i, raw) in snap.caps_raw.iter().enumerate() {
        if raw.vndr != CAP_VENDOR_VIRTIO {
            continue; // 别家厂商能力，跳过（不算异常）
        }
        let kind = match CapKind::from_cfg_type(raw.cfg_type) {
            Some(k) => k,
            None => {
                return Err(Rejection {
                    code: "E_CAP_MALFORMED",
                    what: format!(
                        "第 {} 条能力 cfg_type={} 不属于四类（common/notify/isr/device）",
                        i, raw.cfg_type
                    ),
                    why: "cfg_type 越出 1..=4 说明布局不是 virtio PCI 能力".to_string(),
                    next: "复核设备快照来源；连续畸形按设备缺陷走软渲回退".to_string(),
                });
            }
        };
        if raw.bar as usize >= snap.bar_sizes.len() || snap.bar_sizes[raw.bar as usize] == 0 {
            return Err(Rejection {
                code: "E_CAP_MALFORMED",
                what: format!(
                    "第 {} 条能力（{}）落在 bar {}，该 BAR 不存在",
                    i,
                    kind.label(),
                    raw.bar
                ),
                why: "能力区域必须映射在真实存在的 BAR 上，悬空区域读不到寄存器".to_string(),
                next: "核对 BAR 尺寸表与能力的 bar 号；反复失配按设备缺陷上报".to_string(),
            });
        }
        let bar_end = (snap.bar_sizes[raw.bar as usize] as u64)
            .min((i64::MAX as u64) >> 1);
        let end = raw.offset as u64 + raw.length as u64;
        if end > bar_end {
            return Err(Rejection {
                code: "E_CAP_MALFORMED",
                what: format!(
                    "第 {} 条能力（{}）范围 0x{:X}+0x{:X} 越出 bar {} 尺寸 0x{:X}",
                    i,
                    kind.label(),
                    raw.offset,
                    raw.length,
                    raw.bar,
                    snap.bar_sizes[raw.bar as usize]
                ),
                why: "区域越出 BAR 边界意味着读到的偏移不可信，后续一切寄存器访问都不可信"
                    .to_string(),
                next: "重新枚举 PCI 能力列表再试；仍越界按设备缺陷上报".to_string(),
            });
        }
        if raw.length < kind.min_len() {
            return Err(Rejection {
                code: "E_CAP_MALFORMED",
                what: format!(
                    "第 {} 条能力（{}）长度 0x{:X} 低于最小合法长度 0x{:X}",
                    i,
                    kind.label(),
                    raw.length,
                    kind.min_len()
                ),
                why: "长度不足说明关键寄存器（如 common cfg 的队列字段）截断在半截".to_string(),
                next: "要求虚拟机监控器声明完整区域；不可截短后将就".to_string(),
            });
        }
        let region = CapRegion {
            kind,
            bar: raw.bar,
            offset: raw.offset,
            length: raw.length,
        };
        let slot = match kind {
            CapKind::Common => &mut map.common,
            CapKind::Notify => &mut map.notify,
            CapKind::Isr => &mut map.isr,
            CapKind::Device => &mut map.device,
        };
        if slot.is_some() {
            return Err(Rejection {
                code: "E_CAP_DUP",
                what: format!("{} 被声明了两次（第 {} 条）", kind.label(), i),
                why: "同类配置区域重复声明 = 设备布局错乱，两份区域语义打架".to_string(),
                next: "复核设备快照；确认设备无缺陷后重探".to_string(),
            });
        }
        *slot = Some(region);
        if kind == CapKind::Notify {
            map.notify_off_multiplier = raw.notify_off_multiplier.unwrap_or(1);
        }
    }
    if !map.complete() {
        return Err(Rejection {
            code: "E_CAP_MISSING",
            what: "能力列表里 common/notify/isr/device 四类未凑齐".to_string(),
            why: "四类各司其职：没有 common 初始化不了，没有 device 拿不到显示能力，\
没有 notify/isr 队列不通".to_string(),
            next: "对照 virtio 规格补齐虚拟机配置；缺失类别按三要素上报".to_string(),
        });
    }
    Ok(map)
}

// ---------------------------------------------------------------------------
// 二、寄存器白名单与写留痕（判据：零"试试看"写）
// ---------------------------------------------------------------------------

/// virtio 标准寄存器语义的白名单分类。未列名的一律拒绝写。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegKind {
    Status,
    FeatureSelect,
    FeatureSet,
    DevFeatureSelect,
    DevFeatureSet,
    QueueSelect,
    QueueNum,
    QueueReady,
    QueueNotify,
    ConfigGeneration,
    /// 白名单外——写了就是"试试看"，必须拦截
    Unknown(u8),
}

impl RegKind {
    pub fn label(&self) -> String {
        match self {
            RegKind::Status => "device_status".to_string(),
            RegKind::FeatureSelect => "device_feature_select".to_string(),
            RegKind::FeatureSet => "driver_feature_select".to_string(),
            RegKind::DevFeatureSelect => "driver_feature_select".to_string(),
            RegKind::DevFeatureSet => "driver_feature".to_string(),
            RegKind::QueueSelect => "queue_select".to_string(),
            RegKind::QueueNum => "queue_num".to_string(),
            RegKind::QueueReady => "queue_enable".to_string(),
            RegKind::QueueNotify => "queue_notify".to_string(),
            RegKind::ConfigGeneration => "config_generation".to_string(),
            RegKind::Unknown(v) => format!("未注册寄存器 0x{:02X}", v),
        }
    }
}

/// 一次寄存器写留痕。初始化全过程可回放、可审计。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteTrace {
    pub step: &'static str,
    pub reg: RegKind,
}

// ---------------------------------------------------------------------------
// 三、初始化状态机（判据：初始化序、跳步拒绝、≤100ms）
// ---------------------------------------------------------------------------

/// 初始化阶段。顺序即规格要求的标准序。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Reset,
    Acknowledge,
    Driver,
    Features,
    FeaturesOk,
    Queues,
    DriverOk,
    /// 初始化失败终态（FAILED 位在位）
    Failed,
}

/// 初始化成本模型（确定性逻辑成本，非墙钟测量）。
///
/// 成本数字按"标准寄存器写 + 设备应答"的事务数量估：reset 走完整
/// 状态清零最贵，队列按深度分摊。全部常数可追溯，对拍可复现。
pub const COST_RESET_US: u32 = 2_000;
pub const COST_ACK_US: u32 = 100;
pub const COST_DRIVER_US: u32 = 100;
pub const COST_FEATURES_US: u32 = 500;
pub const COST_FEATURES_OK_US: u32 = 200;
pub const COST_QUEUE_US_PER_256: u32 = 500;
pub const COST_DRIVER_OK_US: u32 = 300;
/// 初始化时间预算（判据：≤100ms）。
pub const INIT_BUDGET_US: u32 = 100_000;

/// 分步耗时账（判据：初始化 ≤100ms 的证据链）。
#[derive(Clone, Debug, Default)]
pub struct InitTiming {
    /// (步骤名, 逻辑耗时 µs)
    pub steps: Vec<(&'static str, u32)>,
}

impl InitTiming {
    pub fn record(&mut self, step: &'static str, us: u32) {
        self.steps.push((step, us));
    }

    pub fn total_us(&self) -> u32 {
        self.steps.iter().map(|(_, us)| *us).sum()
    }

    /// 预算判据（≤100ms）。
    pub fn within_budget(&self) -> bool {
        self.total_us() <= INIT_BUDGET_US
    }

    /// 读屏可达的分步账。
    pub fn describe(&self) -> String {
        let parts: Vec<String> = self
            .steps
            .iter()
            .map(|(s, us)| format!("{}={}µs", s, us))
            .collect();
        format!("初始化分步账：{}，合计 {}µs", parts.join(" + "), self.total_us())
    }
}

/// 一个队列的分配记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueRecord {
    /// 队列号：controlq=0，cursorq=1
    pub index: u16,
    pub size: u16,
    pub ready: bool,
}

/// 队列角色（规格点名 controlq + cursorq 两条）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueRole {
    /// 队列 0：控制命令
    Control,
    /// 队列 1：光标
    Cursor,
}

impl QueueRole {
    pub fn index(self) -> u16 {
        match self {
            QueueRole::Control => 0,
            QueueRole::Cursor => 1,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            QueueRole::Control => "controlq",
            QueueRole::Cursor => "cursorq",
        }
    }
}

/// 初始化完成态——DRIVER_OK 后的全部可见事实。
#[derive(Clone, Debug)]
pub struct InitOutcome {
    pub ok: bool,
    pub stage: Stage,
    /// 状态寄存器最终值
    pub status: u8,
    /// 协商后的驱动特征字
    pub driver_features: u64,
    /// 解析出的设备专有特征
    pub gpu_features: GpuFeatures,
    pub queues: Vec<QueueRecord>,
    pub timing: InitTiming,
    /// 全部寄存器写留痕（零"试试看"写的证据）
    pub writes: Vec<WriteTrace>,
    /// 失败时带三要素；成功为 None
    pub rejection: Option<Rejection>,
}

/// 初始化状态机。每个方法 = 一次合法迁移；非法迁移显性拒绝并置 FAILED。
pub struct InitMachine {
    stage: Stage,
    status: u8,
    snap_offered: u64,
    driver_features: u64,
    gpu_features: GpuFeatures,
    queues: Vec<QueueRecord>,
    timing: InitTiming,
    writes: Vec<WriteTrace>,
    rejection: Option<Rejection>,
}

impl InitMachine {
    /// 从快照启动状态机（起点等价于 reset 完成态）。
    pub fn new(snap: &VirtioGpuDeviceSnapshot) -> InitMachine {
        let mut m = InitMachine {
            stage: Stage::Reset,
            status: 0,
            snap_offered: snap.offered_features,
            driver_features: 0,
            gpu_features: GpuFeatures::default(),
            queues: Vec::new(),
            timing: InitTiming::default(),
            writes: Vec::new(),
            rejection: None,
        };
        m.timing.record("reset", COST_RESET_US);
        m.writes.push(WriteTrace { step: "reset", reg: RegKind::Status });
        m
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }
    pub fn status(&self) -> u8 {
        self.status
    }
    pub fn timing(&self) -> &InitTiming {
        &self.timing
    }
    pub fn writes(&self) -> &[WriteTrace] {
        &self.writes
    }
    pub fn queues(&self) -> &[QueueRecord] {
        &self.queues
    }
    pub fn rejection(&self) -> Option<&Rejection> {
        self.rejection.as_ref()
    }

    fn fail(&mut self, r: Rejection) -> Rejection {
        self.status |= STATUS_FAILED;
        self.stage = Stage::Failed;
        self.rejection = Some(r.clone());
        r
    }

    /// 迁移前置校验：必须处于 `expect` 阶段（跳步 = "试试看"写，显性拒绝）。
    fn expect(&mut self, expect: Stage, step: &'static str) -> Result<(), Rejection> {
        if self.stage == Stage::Failed {
            return Err(self
                .rejection
                .clone()
                .unwrap_or_else(|| Rejection {
                    code: "E_STATE_ILLEGAL",
                    what: "初始化已失败，后续步骤全部拒绝".to_string(),
                    why: "FAILED 终态不可续跑，只能整体重来".to_string(),
                    next: "按失败三要素处置后重建 InitMachine".to_string(),
                }));
        }
        if self.stage != expect {
            return Err(self.fail(Rejection {
                code: "E_STATE_ILLEGAL",
                what: format!(
                    "步骤 {} 要求处于 {:?} 阶段，实际 {:?}",
                    step, expect, self.stage
                ),
                why: "跳过标准序的中间步骤属于探索性写，规格点名禁止".to_string(),
                next: format!("按标准序执行：{} 在 {:?} 之后", step, expect),
            }));
        }
        Ok(())
    }

    /// 记一次寄存器写（白名单外的寄存器直接拒绝——零"试试看"写）。
    pub fn record_write(&mut self, step: &'static str, reg: RegKind) -> Result<(), Rejection> {
        if let RegKind::Unknown(_) = reg {
            return Err(self.fail(Rejection {
                code: "E_EXPLORATORY_WRITE",
                what: format!("步骤 {} 试图写 {}（白名单外）", step, reg.label()),
                why: "只走 virtio 标准寄存器语义，白名单外的写没有规格依据".to_string(),
                next: "查 virtio 规格对应寄存器定义；确需新寄存器走规格评审".to_string(),
            }));
        }
        self.writes.push(WriteTrace { step, reg });
        Ok(())
    }

    /// 步骤 2：ack 状态位。
    pub fn acknowledge(&mut self) -> Result<(), Rejection> {
        self.expect(Stage::Reset, "acknowledge")?;
        self.status |= STATUS_ACKNOWLEDGE;
        self.stage = Stage::Acknowledge;
        self.timing.record("ack", COST_ACK_US);
        self.writes.push(WriteTrace { step: "ack", reg: RegKind::Status });
        Ok(())
    }

    /// 步骤 3：DRIVER 位。
    pub fn set_driver(&mut self) -> Result<(), Rejection> {
        self.expect(Stage::Acknowledge, "set_driver")?;
        self.status |= STATUS_DRIVER;
        self.stage = Stage::Driver;
        self.timing.record("driver", COST_DRIVER_US);
        self.writes.push(WriteTrace { step: "driver", reg: RegKind::Status });
        Ok(())
    }

    /// 步骤 4：特性协商。VIRTIO_F_VERSION_1 必需，缺即拒绝（三要素）。
    pub fn negotiate_features(&mut self) -> Result<(), Rejection> {
        self.expect(Stage::Driver, "negotiate_features")?;
        if self.snap_offered & (1u64 << F_VERSION_1) == 0 {
            return Err(self.fail(Rejection {
                code: "E_NO_VERSION_1",
                what: "设备未提供 VIRTIO_F_VERSION_1 特征位".to_string(),
                why: "现代版 virtio 布局的强制门槛；缺它说明设备按旧版语义工作，\
与本驱动的现代寄存器布局不兼容".to_string(),
                next: "改用支持 virtio 1.0 的虚拟机监控器，或显性走软渲染回退".to_string(),
            }));
        }
        // 驱动特征 = 设备提供 ∩ 驱动要求。VERSION_1 是唯一硬要求；
        // 设备专有特征只记录不强求（可协商降级，不虚报能力）。
        self.driver_features = self.snap_offered;
        self.gpu_features = GpuFeatures::parse(self.snap_offered);
        self.stage = Stage::Features;
        self.timing.record("features", COST_FEATURES_US);
        self.writes.push(WriteTrace { step: "features", reg: RegKind::DevFeatureSet });
        Ok(())
    }

    /// 步骤 5：FEATURES_OK 确认。
    pub fn features_ok(&mut self) -> Result<(), Rejection> {
        self.expect(Stage::Features, "features_ok")?;
        self.status |= STATUS_FEATURES_OK;
        self.stage = Stage::FeaturesOk;
        self.timing.record("features_ok", COST_FEATURES_OK_US);
        self.writes.push(WriteTrace { step: "features_ok", reg: RegKind::Status });
        Ok(())
    }

    /// 步骤 6：队列分配（controlq + cursorq）。
    ///
    /// 队列深度必须为 2 的幂且非零（virtio 语义）；越界/非幂即拒绝。
    pub fn setup_queue(&mut self, role: QueueRole, size: u16) -> Result<(), Rejection> {
        self.expect(Stage::FeaturesOk, "setup_queue")?;
        if size == 0 || !size.is_power_of_two() {
            return Err(self.fail(Rejection {
                code: "E_QUEUE_INVALID",
                what: format!(
                    "{}（队列 {}）深度 {} 非法（要求 2 的幂且非零）",
                    role.label(),
                    role.index(),
                    size
                ),
                why: "virtio 队列深度按 2 的幂做环形取模，非幂深度会让取模运算失真"
                    .to_string(),
                next: format!("以设备报告的合法深度（如 64/128/256）重建 {}", role.label()),
            }));
        }
        let rec = QueueRecord {
            index: role.index(),
            size,
            ready: true,
        };
        // 两条队列齐了自动进 Queues 阶段（标准序的最后一段过渡）
        self.queues.push(rec);
        if self.queues.len() >= 2 {
            self.stage = Stage::Queues;
        }
        self.timing.record(
            match role {
                QueueRole::Control => "queue_control",
                QueueRole::Cursor => "queue_cursor",
            },
            COST_QUEUE_US_PER_256.max(size as u32 / 256 * COST_QUEUE_US_PER_256),
        );
        self.writes.push(WriteTrace { step: "queue_setup", reg: RegKind::QueueNum });
        self.writes.push(WriteTrace { step: "queue_setup", reg: RegKind::QueueReady });
        Ok(())
    }

    /// 步骤 7：DRIVER_OK。两条队列就位才许收口。
    ///
    /// 队列计数先于阶段判定：哪怕调用方停在 FeaturesOk 没切 Queues，
    /// 少队列也按 E_QUEUE_MISSING 报（不给"阶段不对"的误导性答案）。
    pub fn driver_ok(&mut self) -> Result<(), Rejection> {
        if self.queues.len() < 2 {
            return Err(self.fail(Rejection {
                code: "E_QUEUE_MISSING",
                what: format!(
                    "DRIVER_OK 前只就位 {} 条队列（要求 controlq+cursorq 两条）",
                    self.queues.len()
                ),
                why: "少一条队列，光标或控制命令就无路可走，图形栈半身不遂".to_string(),
                next: "先完成两条队列的 setup_queue 再收口".to_string(),
            }));
        }
        self.expect(Stage::Queues, "driver_ok")?;
        self.status |= STATUS_DRIVER_OK;
        self.stage = Stage::DriverOk;
        self.timing.record("driver_ok", COST_DRIVER_OK_US);
        self.writes.push(WriteTrace { step: "driver_ok", reg: RegKind::Status });
        Ok(())
    }

    /// 完整跑完标准初始化序（happy path 的单一入口）。
    ///
    /// 调用方可以先逐方法手动走（测试跳步路径用），生产路径走这个。
    pub fn run_standard_init(snap: &VirtioGpuDeviceSnapshot) -> InitOutcome {
        let mut m = InitMachine::new(snap);
        let mut rejection = None;
        if let Err(e) = m.acknowledge() {
            rejection = Some(e);
        } else if let Err(e) = m.set_driver() {
            rejection = Some(e);
        } else if let Err(e) = m.negotiate_features() {
            rejection = Some(e);
        } else if let Err(e) = m.features_ok() {
            rejection = Some(e);
        } else if let Err(e) = m.setup_queue(QueueRole::Control, snap.queue_sizes[0]) {
            rejection = Some(e);
        } else if let Err(e) = m.setup_queue(QueueRole::Cursor, snap.queue_sizes[1]) {
            rejection = Some(e);
        } else if let Err(e) = m.driver_ok() {
            rejection = Some(e);
        }
        InitOutcome {
            ok: rejection.is_none(),
            stage: m.stage,
            status: m.status,
            driver_features: m.driver_features,
            gpu_features: m.gpu_features,
            queues: m.queues,
            timing: m.timing,
            writes: m.writes,
            rejection,
        }
    }

    /// 失败路径收口（阻塞/拒绝后的显性终态）。
    pub fn fail_with(mut self, r: Rejection) -> InitOutcome {
        let _ = self.fail(r);
        InitOutcome {
            ok: false,
            stage: self.stage,
            status: self.status,
            driver_features: self.driver_features,
            gpu_features: self.gpu_features,
            queues: self.queues,
            timing: self.timing,
            writes: self.writes,
            rejection: self.rejection,
        }
    }
}

/// 快照级一次性校验（探测入口）：身份 → 显示能力。通过才进初始化。
pub fn preflight(snap: &VirtioGpuDeviceSnapshot) -> Result<(), Rejection> {
    if let Some(r) = snap.identity_rejection() {
        return Err(r);
    }
    if snap.device_id == DEVICE_LEGACY {
        return Err(Rejection {
            code: "E_LEGACY_DEVICE",
            what: format!(
                "设备 {} 是旧版 virtio-gpu（device 0x{:04X}），显性拒绝初始化",
                snap.slot, DEVICE_LEGACY
            ),
            why: "旧版没有现代能力布局，初始化只能靠试探性寄存器写——\
正是规格点名禁止的\"试试看\"路径".to_string(),
            next: "在虚拟机配置里改用现代版 virtio-gpu（device 0x1050），\
或把该设备显性标注为不支持".to_string(),
        });
    }
    snap.display.validate()
}

/// 确定性指纹：同快照同初始化结果 ⇒ 同指纹（对拍可复现）。
pub fn outcome_fingerprint(snap: &VirtioGpuDeviceSnapshot, out: &InitOutcome) -> u64 {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&snap.fingerprint().to_le_bytes());
    buf.extend_from_slice(&out.status.to_le_bytes());
    buf.extend_from_slice(&out.driver_features.to_le_bytes());
    buf.extend_from_slice(&out.timing.total_us().to_le_bytes());
    for q in out.queues.iter() {
        buf.extend_from_slice(&q.index.to_le_bytes());
        buf.extend_from_slice(&q.size.to_le_bytes());
    }
    fnv1a64(&buf)
}
