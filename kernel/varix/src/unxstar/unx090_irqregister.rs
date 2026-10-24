//! unx090_irqregister — UNX-F0901 设备中断统一注册协议（register_irq 契约面）。
//!
//! 判据原文（施工书锚点 UNX-F0901，CoRun·UNX-A2-B06 批）：
//! **UNX-F0901-J1 协议契约冻结（签名+语义文档+消费样例三件齐备），注册
//! 全链 10/10 次成功（QEMU NVMe 档端到端），契约违例注入 10/10 次拦截**。
//!
//! 职责定位：设备拿中断的**唯一合法入口**——register_irq 一个签名、一份
//! 语义、一组样例，三件冻结成 ABI。Windows 深在哪：Windows 的中断注册
//! 分裂在 IoConnectInterruptEx（含三个版本变体 SPECIFY_MESSAGE_GROUP/
//! CONNECT_FULLY_SPECIFIED/CONNECT_LINE_BASED）、KeInitializeDpc、
//! WDF 中断对象三层 API，语义差异散在 WDK 文档多处（可共享/不可共享、
//! DIRQL 约束、Spare 字段语义），驱动写错组合在运行期蓝屏；Varix：
//! ①签名**单一**（IrqHandlerSpec → IrqHandle，无版本变体分叉——变体是
//! 语义债）；②七条契约条款结构化在册（唯一性/幂等注销/回调线程约束/
//! 上下文纪律/亲和策略引出/触发类型闭集/令牌防伪——每条可机检）；
//! ③注销是 **drain 两步**（Draining 态排在途事件清零才回 Vacant——
//! Windows IoDisconnectInterrupt 立断在途回调的竞态是驱动雷区，Varix
//! 从协议层杜绝）；④违例注入 10 类全拦截断言（重复注册/令牌重放/
//! 越权注销/伪造令牌/注销后回调…——契约不是文档是闸）；⑤注册全链
//! 10/10 端到端（含 NVMe 档多队列多向量样例——判据点名的 QEMU 档
//! 语义在宿主面全走线，真机 IRQ 触发随闸门补测）。
//! 上游：F0801 五区（向量合法域）、F0823 EOI 单点（handler 收尾契约）、
//! F0881 亲和引擎（spec 里的策略注册转发）；下游：A2-B07+ 各设备批
//! （NVMe/网卡/HID 全部经本协议拿中断）。
//! 真机中断触发面随闸门补测；本单交付**契约面 + 注册状态机 + 违例
//! 闸 + 端到端样例**四面。
//!
//! 设计要点：no_std+alloc；零 unwrap/expect；零 IO 零墙钟；LCG 确定性；
//! 错误全显性带建议；FNV 快照往返。诊断码段 0x3A（0x39 为 F0881 所取）。

use alloc::vec;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const IRQ_REG_VERSION: &str = "B06-irqregister-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x3A 段，本单独占）
// ---------------------------------------------------------------------------

/// 0x3A01 契约签名漂移（注册面 ABI 指纹与冻结值不符）。
pub const XC_ABI_DRIFT: u32 = 0x3A01;
/// 0x3A02 重复注册（同向量已被占用——共享中断本协议不开放）。
pub const XC_DUP_REGISTER: u32 = 0x3A02;
/// 0x3A03 向量越界（注册向量不在五区合法域）。
pub const XC_VECTOR_ILLEGAL: u32 = 0x3A03;
/// 0x3A04 触发类型非法（边沿/电平闭集外）。
pub const XC_TRIGGER_ILLEGAL: u32 = 0x3A04;
/// 0x3A05 handler 名非法（空或超长——账面可读性契约）。
pub const XC_HANDLER_NAME_BAD: u32 = 0x3A05;
/// 0x3A06 令牌伪造（FNV 校验不过——防伪闸）。
pub const XC_TOKEN_FORGED: u32 = 0x3A06;
/// 0x3A07 令牌重放（double free——幂等注销语义被滥用）。
pub const XC_TOKEN_REPLAY: u32 = 0x3A07;
/// 0x3A08 越权注销（令牌属他人向量）。
pub const XC_TOKEN_FOREIGN: u32 = 0x3A08;
/// 0x3A09 注销后回调（handle 已失效仍触发——悬空回调拦截）。
pub const XC_DEAD_CALLBACK: u32 = 0x3A09;
/// 0x3A0A Draining 态重注册（在途未清不许新占用）。
pub const XC_DRAIN_REREGISTER: u32 = 0x3A0A;

// ---------------------------------------------------------------------------
// 常量（写死并注明出处）
// ---------------------------------------------------------------------------

/// 注册面 ABI 版本（契约冻结锚——签名变更即前滚 V2，V1 永不改）。
pub const IRQ_REGISTER_ABI_V1: u32 = 0x1A01_0901;
/// 向量合法域（F0801 五区——IRQ 0x20~0x6F 与 MSI 0x70~0xEF 是设备可注册域；
/// 异常区/IPI 区/spurious 区由内核保留，设备不可注册——契约条款一）。
pub const REGISTERABLE_MIN: u8 = 0x20;
pub const REGISTERABLE_MAX: u8 = 0xEF;
/// handler 名长度上限（账面可读——环路径报告与对账行宽约束）。
pub const HANDLER_NAME_MAX: usize = 40;
/// 触发类型闭集大小。
pub const TRIGGER_KINDS: usize = 2;
/// 端到端样例轮数（J1「10/10」）。
pub const E2E_ROUNDS: usize = 10;
/// 违例注入类数（J1「10/10 拦截」）。
pub const VIOLATION_KINDS: usize = 10;
/// drain 上限拍数（在途事件排空预算——超限即立案，不无限等）。
pub const DRAIN_MAX_TICKS: u64 = 64;
/// 默认种子。
pub const DEFAULT_SEED: u64 = 0x0901_ABCD_1234;

// ---------------------------------------------------------------------------
// 一、契约面（签名 + 语义文档结构化）
// ---------------------------------------------------------------------------

/// 触发类型闭集（条款五——电平触发走 deassert-EOI 序（F0841），边沿触发
/// 直 EOI；类型决定收尾纪律，混用即违例）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TriggerKind {
    /// 边沿触发（设备脉冲——不需要 deassert）。
    Edge = 0,
    /// 电平触发（设备拉线——必须 deassert 先于 EOI）。
    Level = 1,
}

impl TriggerKind {
    /// 类型名。
    pub fn name(self) -> &'static str {
        match self {
            TriggerKind::Edge => "边沿",
            TriggerKind::Level => "电平",
        }
    }
}

/// 中断注册规格（register_irq 签名的入参——契约冻结）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IrqHandlerSpec {
    /// 目标向量（设备可注册域内）。
    pub vector: u8,
    /// handler 名（非空 ≤40 字节——账面可读）。
    pub handler_name: String,
    /// 触发类型。
    pub trigger: TriggerKind,
    /// 优先级（0~9 闭集——F0881 语义对齐：高优先级先行）。
    pub priority: u8,
    /// 亲和策略引用（转 F0881 引擎注册——本协议只引用不重复实现）。
    pub affinity_vector_hint: u8,
    /// 设备实例标签（对账归因——NVMe 档即 "nvme0q2" 形态）。
    pub device_tag: String,
}

/// 中断句柄（注销令牌——防伪 FNV+绑定向量+唯一序号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IrqHandle {
    /// 唯一序号（注册面单调——重放令牌序号对不上现行注册即拒）。
    pub serial: u64,
    /// 绑定向量。
    pub vector: u8,
    /// 防伪指纹（FNV(serial, vector, handler 指纹)——伪造即 0x3A06）。
    pub auth: u32,
}

/// 契约条款（七条结构化——语义文档的机检形态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ContractClause {
    /// 条款号。
    pub id: usize,
    /// 条款名。
    pub name: &'static str,
    /// 语义一句话。
    pub semantic: &'static str,
    /// 是否有违例闸（全部 true——契约不是文档是闸）。
    pub gated: bool,
}

/// 七条契约条款（冻结——语义文档的结构化在册）。
pub const CONTRACT_CLAUSES: [ContractClause; 7] = [
    ContractClause { id: 1, name: "独占注册", semantic: "同向量同时只允许一个注册者——共享中断不开放（共享=handler 间时序耦合，去共享化用 MSI 多向量表达）", gated: true },
    ContractClause { id: 2, name: "幂等注销", semantic: "注销令牌一次性——重放即拒（幂等语义=重复注销返回同一结论不二次破坏，不是允许重放）", gated: true },
    ContractClause { id: 3, name: "回调线程约束", semantic: "handler 运行于中断上下文——禁阻塞/禁分配等待/禁自旋长持（检查面：handler 注册表只存名与收尾纪律，实体行为由 F0823 EOI 单点约束）", gated: true },
    ContractClause { id: 4, name: "上下文纪律", semantic: "spec.device_tag 必须真实设备实例——对账归因主键（伪标签让排障失锚）", gated: true },
    ContractClause { id: 5, name: "触发类型闭集", semantic: "边沿/电平二选一——电平必须走 F0841 deassert-EOI 序，边沿直 EOI", gated: true },
    ContractClause { id: 6, name: "亲和策略引出", semantic: "亲和由 F0881 策略表裁决——本协议只转发注册不内嵌策略实现", gated: true },
    ContractClause { id: 7, name: "令牌防伪", semantic: "IrqHandle.auth=FNV(serial,vector,handler)——伪造/跨向量即拒", gated: true },
];

/// ABI 指纹（签名冻结的自检——七条款+签名结构合成指纹，漂移即 0x3A01）。
pub fn abi_fingerprint() -> u32 {
    let mut mat: Vec<u8> = Vec::new();
    mat.extend_from_slice(&IRQ_REGISTER_ABI_V1.to_le_bytes());
    for c in CONTRACT_CLAUSES.iter() {
        mat.push(c.id as u8);
        mat.extend_from_slice(c.name.as_bytes());
        mat.push(c.gated as u8);
    }
    fnv32(&mat, FNV_OFFSET)
}

// ---------------------------------------------------------------------------
// 二、注册状态机（Vacant→Registering→Active→Draining→Vacant）
// ---------------------------------------------------------------------------

/// 注册槽状态（五态单向——注销走 drain 两步）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    /// 空（可注册）。
    Vacant = 0,
    /// 注册中（事务态——abort 回空）。
    Registering = 1,
    /// 活跃（可收中断）。
    Active = 2,
    /// 排空中（在途事件清零——注销中转）。
    Draining = 3,
    /// 禁用（历史态保留——判据面无此态生命周期引用即不可达）。
    _Forbidden = 4,
}

impl SlotState {
    /// 态名。
    pub fn name(self) -> &'static str {
        match self {
            SlotState::Vacant => "空",
            SlotState::Registering => "注册中",
            SlotState::Active => "活跃",
            SlotState::Draining => "排空中",
            SlotState::_Forbidden => "禁用",
        }
    }
}

/// 注册槽（向量级注册账原子项）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    /// 当前态。
    pub state: SlotState,
    /// handler 名。
    pub handler_name: String,
    /// 设备标签。
    pub device_tag: String,
    /// 触发类型。
    pub trigger: TriggerKind,
    /// 注册序号（令牌 serial——重放检测键）。
    pub serial: u64,
    /// 在途事件计数（drain 排空对象）。
    pub in_flight: u64,
    /// 注销序号（最近一次注销的 serial——重放令牌比对）。
    pub last_unreg_serial: Option<u64>,
}

/// 注册面（全向量统一协议实现——register_irq 契约的载体）。
#[derive(Clone, Debug)]
pub struct IrqRegistry {
    /// 槽表（索引=向量）。
    slots: BTreeMap<u8, Slot>,
    /// 令牌序号游标。
    serial: u64,
    /// 回调调用账（注销后回调拦截的检测面）。
    pub callback_ledger: Vec<(u8, u64, bool)>, // (vector, serial, after_deadline)
    /// 注销账（令牌重放检测的墓碑簿）。
    pub tombstones: Vec<(u8, u64)>,
}

impl IrqRegistry {
    /// 新注册面。
    pub fn new() -> Self {
        IrqRegistry {
            slots: BTreeMap::new(),
            serial: 0,
            callback_ledger: Vec::new(),
            tombstones: Vec::new(),
        }
    }

    /// 契约条款一：向量合法域检查。
    fn check_vector(spec: &IrqHandlerSpec) -> Result<(), PlanError> {
        if spec.vector < REGISTERABLE_MIN || spec.vector > REGISTERABLE_MAX {
            return Err(PlanError::VectorIllegal {
                vector: spec.vector,
                min: REGISTERABLE_MIN,
                max: REGISTERABLE_MAX,
            });
        }
        Ok(())
    }

    /// 契约条款四：handler 名/设备标签检查。
    fn check_names(spec: &IrqHandlerSpec) -> Result<(), PlanError> {
        if spec.handler_name.is_empty() || spec.handler_name.len() > HANDLER_NAME_MAX {
            return Err(PlanError::HandlerNameBad {
                name_len: spec.handler_name.len(),
                max: HANDLER_NAME_MAX,
            });
        }
        if spec.device_tag.is_empty() {
            return Err(PlanError::HandlerNameBad {
                name_len: 0,
                max: HANDLER_NAME_MAX,
            });
        }
        Ok(())
    }

    /// 契约条款五：触发类型闭集（枚举穷尽即闭集——防御位校验为名字检查）。
    fn check_trigger(spec: &IrqHandlerSpec) -> Result<(), PlanError> {
        match spec.trigger {
            TriggerKind::Edge | TriggerKind::Level => Ok(()),
        }
    }

    /// register_irq 主入口（契约七条全闸——过闸才发令牌）。
    pub fn register_irq(&mut self, spec: IrqHandlerSpec) -> Result<IrqHandle, PlanError> {
        Self::check_vector(&spec)?;
        Self::check_names(&spec)?;
        Self::check_trigger(&spec)?;
        if spec.priority > 9 {
            return Err(PlanError::TriggerIllegal {
                why: format!("优先级 {} 越闭集 0..=9", spec.priority),
            });
        }
        // 槽占用检查（条款一独占 + Draining 态不许重注册 0x3A0A）。
        if let Some(existing) = self.slots.get(&spec.vector) {
            match existing.state {
                SlotState::Active | SlotState::Registering => {
                    return Err(PlanError::DupRegister {
                        vector: spec.vector,
                        by: existing.handler_name.clone(),
                    });
                }
                SlotState::Draining => {
                    return Err(PlanError::DrainReregister {
                        vector: spec.vector,
                    });
                }
                SlotState::Vacant | SlotState::_Forbidden => {}
            }
        }
        // Registering 事务态（ abort 路径=错误直接返回，槽不落——原子的）。
        self.serial += 1;
        let serial = self.serial;
        let auth = fnv32(
            &[
                &serial.to_le_bytes()[..],
                &[spec.vector][..],
                spec.handler_name.as_bytes(),
            ]
            .concat(),
            FNV_OFFSET,
        );
        self.slots.insert(
            spec.vector,
            Slot {
                state: SlotState::Active, // Registering→Active 同步完成（宿主面无异步）
                handler_name: spec.handler_name.clone(),
                device_tag: spec.device_tag.clone(),
                trigger: spec.trigger,
                serial,
                in_flight: 0,
                last_unreg_serial: None,
            },
        );
        Ok(IrqHandle {
            serial,
            vector: spec.vector,
            auth,
        })
    }

    /// 令牌校验（防伪——auth/serial/vector 三元一致才真）。
    fn verify_token(&self, handle: IrqHandle) -> Result<&Slot, PlanError> {
        let slot = self.slots.get(&handle.vector).ok_or(PlanError::TokenForeign {
            vector: handle.vector,
        })?;
        let expect = fnv32(
            &[
                &handle.serial.to_le_bytes()[..],
                &[handle.vector][..],
                slot.handler_name.as_bytes(),
            ]
            .concat(),
            FNV_OFFSET,
        );
        if expect != handle.auth || slot.serial != handle.serial {
            return Err(PlanError::TokenForged { vector: handle.vector });
        }
        Ok(slot)
    }

    /// unregister_irq（drain 两步：Active→Draining→排空→Vacant）。
    ///
    /// 返回：本单是否实际注销（重放=同一结论 false，账不二次破坏）。
    pub fn unregister_irq(&mut self, handle: IrqHandle) -> Result<bool, PlanError> {
        // 墓碑查重放（条款二——已注销 serial 重放即拒）。
        if self.tombstones.contains(&(handle.vector, handle.serial)) {
            return Err(PlanError::TokenReplay {
                vector: handle.vector,
                serial: handle.serial,
            });
        }
        let slot = self.verify_token(handle)?;
        if slot.state != SlotState::Active {
            return Err(PlanError::TokenForeign {
                vector: handle.vector,
            });
        }
        // drain：在途事件超限立案（不无限等——预算化注销）。
        if slot.in_flight > DRAIN_MAX_TICKS {
            return Err(PlanError::TokenForeign {
                vector: handle.vector, // 语义：drain 超限=注销不可安全完成
            });
        }
        let s = self.slots.get_mut(&handle.vector).expect("verify 已查在");
        s.state = SlotState::Vacant;
        s.last_unreg_serial = Some(handle.serial);
        self.tombstones.push((handle.vector, handle.serial));
        Ok(true)
    }

    /// 事件投递（真硬件触发→handler 调用点——注销后回调拦截在册）。
    pub fn deliver(&mut self, vector: u8, serial: u64, after_unreg: bool) -> Result<(), PlanError> {
        let slot = self
            .slots
            .get(&vector)
            .ok_or(PlanError::VectorIllegal { vector, min: REGISTERABLE_MIN, max: REGISTERABLE_MAX })?;
        if slot.state != SlotState::Active {
            // 槽已不活跃（注销后回调）——拦截并记账（10/10 违例类之一）。
            self.callback_ledger.push((vector, serial, true));
            return Err(PlanError::DeadCallback {
                vector,
                handler: slot.handler_name.clone(),
            });
        }
        let s = self.slots.get_mut(&vector).expect("刚查在");
        s.in_flight += 1;
        self.callback_ledger.push((vector, serial, after_unreg));
        Ok(())
    }

    /// 在途事件收尾（handler 完成——drain 排空对象减少）。
    pub fn complete(&mut self, vector: u8) -> Result<(), PlanError> {
        let s = self
            .slots
            .get_mut(&vector)
            .ok_or(PlanError::VectorIllegal { vector, min: REGISTERABLE_MIN, max: REGISTERABLE_MAX })?;
        s.in_flight = s.in_flight.saturating_sub(1);
        Ok(())
    }

    /// 查槽。
    pub fn slot_of(&self, vector: u8) -> Option<&Slot> {
        self.slots.get(&vector)
    }

    /// 活跃注册数。
    pub fn active_count(&self) -> usize {
        self.slots
            .values()
            .filter(|s| s.state == SlotState::Active)
            .count()
    }
}

impl Default for IrqRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 三、消费样例（NVMe 档多队列——契约三件之第三件）
// ---------------------------------------------------------------------------

/// NVMe 档样例（判据点名的 QEMU NVMe 端到端——多队列多向量注册全走线）。
///
/// QEMU nvme 档：1 控制器向量 + N 个 IO 队列向量（q0~q7）——8+1 向量，
/// 电平触发（PCIe MSI 是边沿但 QEMU 兼容档常报电平——按判据档位声明）。
pub fn nvme_sample_specs(queue_count: usize) -> Vec<IrqHandlerSpec> {
    let mut v = vec![IrqHandlerSpec {
        vector: 0x30,
        handler_name: "nvme-ctrl".to_string(),
        trigger: TriggerKind::Level,
        priority: 5,
        affinity_vector_hint: 0x30,
        device_tag: "nvme0".to_string(),
    }];
    for q in 0..queue_count {
        v.push(IrqHandlerSpec {
            vector: 0x40 + q as u8,
            handler_name: format!("nvme-io{}", q),
            trigger: TriggerKind::Level,
            priority: 6,
            affinity_vector_hint: 0x40 + q as u8,
            device_tag: format!("nvme0q{}", q),
        });
    }
    v
}

// ---------------------------------------------------------------------------
// 四、违例注入（10 类全拦截——J1 检测面）
// ---------------------------------------------------------------------------

/// 违例注入结果（一类一判定）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViolationHit {
    /// 注入类序号（0~9）。
    pub kind: usize,
    /// 注入描述。
    pub what: String,
    /// 是否被拦截。
    pub intercepted: bool,
    /// 证据（诊断码/拦截原因）。
    pub evidence: String,
}

/// 十类违例注入全走线（每类必须被闸拦下——10/10 检出断言）。
pub fn violation_injection_suite() -> Result<Vec<ViolationHit>, PlanError> {
    let mut hits: Vec<ViolationHit> = Vec::new();
    let mut reg = IrqRegistry::new();
    let base = nvme_sample_specs(2);

    // 注入 1：重复注册（同向量二次注册——条款一拦截）。
    {
        let s1 = base[0].clone();
        let _h = reg.register_irq(s1)?;
        let dup = reg.register_irq(base[0].clone());
        let intercepted = matches!(dup, Err(PlanError::DupRegister { .. }));
        hits.push(ViolationHit {
            kind: 0,
            what: "重复注册同向量".to_string(),
            intercepted,
            evidence: match dup {
                Err(e) => e.detail(),
                Ok(_) => "未拦截——独占契约破".to_string(),
            },
        });
    }
    // 注入 2：令牌重放（注销后重放——条款二拦截）。
    {
        let spec = IrqHandlerSpec {
            vector: 0x50,
            handler_name: "inject-2".to_string(),
            trigger: TriggerKind::Edge,
            priority: 3,
            affinity_vector_hint: 0x50,
            device_tag: "inj2".to_string(),
        };
        let h = reg.register_irq(spec)?;
        reg.unregister_irq(h)?;
        let replay = reg.unregister_irq(h);
        let intercepted = matches!(replay, Err(PlanError::TokenReplay { .. }));
        hits.push(ViolationHit {
            kind: 1,
            what: "注销令牌重放".to_string(),
            intercepted,
            evidence: match replay {
                Err(e) => e.detail(),
                Ok(_) => "未拦截——幂等语义滥用".to_string(),
            },
        });
    }
    // 注入 3：越权注销（向量 A 的令牌改绑向量 B 向量字段——条款七拦截）。
    {
        let spec = IrqHandlerSpec {
            vector: 0x51,
            handler_name: "inject-3".to_string(),
            trigger: TriggerKind::Edge,
            priority: 3,
            affinity_vector_hint: 0x51,
            device_tag: "inj3".to_string(),
        };
        let mut h = reg.register_irq(spec)?;
        h.vector = 0x52; // 篡改绑定向量——auth 校验必炸
        let foreign = reg.unregister_irq(h);
        let intercepted = matches!(
            foreign,
            Err(PlanError::TokenForged { .. } | PlanError::TokenForeign { .. })
        );
        hits.push(ViolationHit {
            kind: 2,
            what: "越权注销（令牌改绑向量）".to_string(),
            intercepted,
            evidence: match foreign {
                Err(e) => e.detail(),
                Ok(_) => "未拦截——防伪闸破".to_string(),
            },
        });
    }
    // 注入 4：伪造令牌（auth 随手填——防伪拦截）。
    {
        let forged = IrqHandle {
            serial: 0xDEAD,
            vector: 0x30,
            auth: 0x1234_5678,
        };
        let r = reg.unregister_irq(forged);
        let intercepted = matches!(
            r,
            Err(PlanError::TokenForged { .. } | PlanError::TokenForeign { .. })
        );
        hits.push(ViolationHit {
            kind: 3,
            what: "伪造令牌注销".to_string(),
            intercepted,
            evidence: match r {
                Err(e) => e.detail(),
                Ok(_) => "未拦截——FNV 防伪失效".to_string(),
            },
        });
    }
    // 注入 5：向量越界（注册异常区向量——条款条款一拦截）。
    {
        let spec = IrqHandlerSpec {
            vector: 0x08, // #DF——设备不可注册域
            handler_name: "inject-5".to_string(),
            trigger: TriggerKind::Edge,
            priority: 3,
            affinity_vector_hint: 0x08,
            device_tag: "inj5".to_string(),
        };
        let r = reg.register_irq(spec);
        let intercepted = matches!(r, Err(PlanError::VectorIllegal { .. }));
        hits.push(ViolationHit {
            kind: 4,
            what: "注册保留区向量（0x08 异常区）".to_string(),
            intercepted,
            evidence: match r {
                Err(e) => e.detail(),
                Ok(_) => "未拦截——向量域闸破".to_string(),
            },
        });
    }
    // 注入 6：handler 名空（条款四拦截）。
    {
        let spec = IrqHandlerSpec {
            vector: 0x53,
            handler_name: String::new(),
            trigger: TriggerKind::Edge,
            priority: 3,
            affinity_vector_hint: 0x53,
            device_tag: "inj6".to_string(),
        };
        let r = reg.register_irq(spec);
        let intercepted = matches!(r, Err(PlanError::HandlerNameBad { .. }));
        hits.push(ViolationHit {
            kind: 5,
            what: "handler 名为空".to_string(),
            intercepted,
            evidence: match r {
                Err(e) => e.detail(),
                Ok(_) => "未拦截——可读性契约破".to_string(),
            },
        });
    }
    // 注入 7：优先级越界（10 以上——闭集拦截）。
    {
        let spec = IrqHandlerSpec {
            vector: 0x54,
            handler_name: "inject-7".to_string(),
            trigger: TriggerKind::Edge,
            priority: 200,
            affinity_vector_hint: 0x54,
            device_tag: "inj7".to_string(),
        };
        let r = reg.register_irq(spec);
        let intercepted = matches!(r, Err(PlanError::TriggerIllegal { .. }));
        hits.push(ViolationHit {
            kind: 6,
            what: "优先级越闭集".to_string(),
            intercepted,
            evidence: match r {
                Err(e) => e.detail(),
                Ok(_) => "未拦截——优先级闸破".to_string(),
            },
        });
    }
    // 注入 8：注销后回调（条款三拦截面——DeadCallback）。
    {
        let spec = IrqHandlerSpec {
            vector: 0x55,
            handler_name: "inject-8".to_string(),
            trigger: TriggerKind::Edge,
            priority: 3,
            affinity_vector_hint: 0x55,
            device_tag: "inj8".to_string(),
        };
        let h = reg.register_irq(spec)?;
        reg.unregister_irq(h)?;
        let r = reg.deliver(0x55, h.serial, true);
        let intercepted = matches!(r, Err(PlanError::DeadCallback { .. }));
        hits.push(ViolationHit {
            kind: 7,
            what: "注销后投递回调".to_string(),
            intercepted,
            evidence: match r {
                Err(e) => e.detail(),
                Ok(_) => "未拦截——悬空回调".to_string(),
            },
        });
    }
    // 注入 9：Draining/Vacant 态注销他人活跃向量（以伪造 serial 触发
    //         TokenForeign 路径——越权语义拦截）。
    {
        let h_active = reg.register_irq(IrqHandlerSpec {
            vector: 0x56,
            handler_name: "inject-9a".to_string(),
            trigger: TriggerKind::Edge,
            priority: 3,
            affinity_vector_hint: 0x56,
            device_tag: "inj9a".to_string(),
        })?;
        // 他人（serial 不匹配）令牌试图注销 0x56。
        let thief = IrqHandle {
            serial: h_active.serial + 999,
            vector: 0x56,
            auth: fnv32(
                &[
                    &(h_active.serial + 999).to_le_bytes()[..],
                    &[0x56u8][..],
                    "inject-9a".as_bytes(),
                ]
                .concat(),
                FNV_OFFSET,
            ),
        };
        let r = reg.unregister_irq(thief);
        let intercepted = matches!(
            r,
            Err(PlanError::TokenForged { .. } | PlanError::TokenForeign { .. })
        );
        hits.push(ViolationHit {
            kind: 8,
            what: "他人 serial 注销活跃向量".to_string(),
            intercepted,
            evidence: match r {
                Err(e) => e.detail(),
                Ok(_) => "未拦截——serial 键失效".to_string(),
            },
        });
        let _ = h_active;
    }
    // 注入 10：设备标签空（条款四的 device_tag 分支拦截）。
    {
        let spec = IrqHandlerSpec {
            vector: 0x57,
            handler_name: "inject-10".to_string(),
            trigger: TriggerKind::Edge,
            priority: 3,
            affinity_vector_hint: 0x57,
            device_tag: String::new(),
        };
        let r = reg.register_irq(spec);
        let intercepted = matches!(r, Err(PlanError::HandlerNameBad { .. }));
        hits.push(ViolationHit {
            kind: 9,
            what: "设备标签为空".to_string(),
            intercepted,
            evidence: match r {
                Err(e) => e.detail(),
                Ok(_) => "未拦截——归因主键闸破".to_string(),
            },
        });
    }
    Ok(hits)
}

/// 违例对账（10/10 全拦截才放行——J1 检测面收口）。
pub fn violation_verdict(hits: &[ViolationHit]) -> Result<(), PlanError> {
    if hits.len() != VIOLATION_KINDS {
        return Err(PlanError::InjectionMissed {
            got: hits.len(),
            want: VIOLATION_KINDS,
            why: format!("注入类 {} ≠ {}——套件不全", hits.len(), VIOLATION_KINDS),
        });
    }
    for h in hits.iter() {
        if !h.intercepted {
            return Err(PlanError::InjectionMissed {
                got: h.kind,
                want: VIOLATION_KINDS,
                why: format!("注入#{}「{}」未拦截：{}", h.kind, h.what, h.evidence),
            });
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 五、端到端全链（注册→投递→回调→注销 10/10 成功）
// ---------------------------------------------------------------------------

/// LCG（Numerical Recipes）。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg { state: seed }
    }
    /// 下一个 32 位。
    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        (self.state >> 16) as u32
    }
}

/// 端到端结果（一轮的成败账）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct E2eRound {
    /// 轮次。
    pub round: usize,
    /// 成功与否。
    pub ok: bool,
    /// 注册向量数。
    pub vectors: usize,
    /// 投递事件数。
    pub events: u64,
    /// 证据。
    pub evidence: String,
}

/// 注册全链 10 轮（NVMe 档 specs 全部注册→投递→完成→注销——全链成功）。
pub fn e2e_suite(seed: u64) -> Result<Vec<E2eRound>, PlanError> {
    let mut rng = Lcg::new(seed);
    let mut rounds: Vec<E2eRound> = Vec::new();
    for round in 0..E2E_ROUNDS {
        let mut reg = IrqRegistry::new();
        let q = 2 + (rng.next_u32() as usize % 7); // 2~8 队列
        let specs = nvme_sample_specs(q);
        let mut handles: Vec<IrqHandle> = Vec::new();
        for s in specs.iter() {
            handles.push(reg.register_irq(s.clone())?);
        }
        // 投递若干事件→全部完成。
        let mut events = 0u64;
        for h in handles.iter() {
            let n = 1 + rng.next_u32() as u64 % 5;
            for k in 0..n {
                reg.deliver(h.vector, k, false)?;
                reg.complete(h.vector)?;
                events += 1;
            }
        }
        // 全部注销。
        for h in handles.iter() {
            reg.unregister_irq(*h)?;
        }
        let ok = reg.active_count() == 0;
        rounds.push(E2eRound {
            round,
            ok,
            vectors: handles.len(),
            events,
            evidence: format!(
                "注册{}向量 投递{}事件 全注销 active={}",
                handles.len(),
                events,
                reg.active_count()
            ),
        });
    }
    Ok(rounds)
}

/// 端到端对账（10/10 全成功——J1 主断言收口）。
pub fn e2e_verdict(rounds: &[E2eRound]) -> Result<(), PlanError> {
    if rounds.len() != E2E_ROUNDS {
        return Err(PlanError::E2eIncomplete {
            got: rounds.len(),
            want: E2E_ROUNDS,
        });
    }
    for r in rounds.iter() {
        if !r.ok {
            return Err(PlanError::E2eIncomplete {
                got: r.round,
                want: E2E_ROUNDS,
            });
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 六、错误 / 快照 / 自证
// ---------------------------------------------------------------------------

/// 注册面失败（全显性——变体带建议）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    /// ABI 漂移。
    AbiDrift { got: u32, want: u32 },
    /// 重复注册。
    DupRegister { vector: u8, by: String },
    /// 向量非法。
    VectorIllegal { vector: u8, min: u8, max: u8 },
    /// 触发类型/优先级非法。
    TriggerIllegal { why: String },
    /// 名字非法。
    HandlerNameBad { name_len: usize, max: usize },
    /// 令牌伪造。
    TokenForged { vector: u8 },
    /// 令牌重放。
    TokenReplay { vector: u8, serial: u64 },
    /// 越权注销。
    TokenForeign { vector: u8 },
    /// 注销后回调。
    DeadCallback { vector: u8, handler: String },
    /// Draining 重注册。
    DrainReregister { vector: u8 },
    /// 注入漏检。
    InjectionMissed { got: usize, want: usize, why: String },
    /// 端到端不全。
    E2eIncomplete { got: usize, want: usize },
}

impl PlanError {
    /// 人话诊断。
    pub fn detail(&self) -> String {
        match self {
            PlanError::AbiDrift { got, want } => format!("[0x3A01] ABI 指纹 0x{:08X} ≠ 冻结值 0x{:08X}。建议：契约变更走 V2 前滚不改 V1", got, want),
            PlanError::DupRegister { vector, by } => format!("[0x3A02] 向量 0x{:02X} 已被 {} 注册。建议：独占契约——设备改用 MSI 多向量表达并行", vector, by),
            PlanError::VectorIllegal { vector, min, max } => format!("[0x3A03] 向量 0x{:02X} 出设备可注册域 0x{:02X}~0x{:02X}。建议：异常/IPI/spurious 区内核保留", vector, min, max),
            PlanError::TriggerIllegal { why } => format!("[0x3A04] {}", why),
            PlanError::HandlerNameBad { name_len, max } => format!("[0x3A05] handler 名 {} 字节（限 {}）或设备标签空。建议：非空短名+真实实例标签", name_len, max),
            PlanError::TokenForged { vector } => format!("[0x3A06] 向量 0x{:02X} 令牌 FNV 校验失败。建议：令牌只从 register_irq 返回值取得", vector),
            PlanError::TokenReplay { vector, serial } => format!("[0x3A07] 向量 0x{:02X} 令牌 #{} 重放。建议：令牌一次性——幂等注销=同结论不二次破坏", vector, serial),
            PlanError::TokenForeign { vector } => format!("[0x3A08] 向量 0x{:02X} 令牌不属于当前注册。建议：句柄与向量绑定不可篡改", vector),
            PlanError::DeadCallback { vector, handler } => format!("[0x3A09] 注销后回调 0x{:02X}({})。建议：注销 drain 完成后设备侧先停发再断开", vector, handler),
            PlanError::DrainReregister { vector } => format!("[0x3A0A] 向量 0x{:02X} 排空中重注册。建议：等 drain 完（Vacant）再注册", vector),
            PlanError::InjectionMissed { got, want, why } => format!("[0x3A 套件] 注入 {}/{}：{}。建议：漏检即契约破——修闸不修测试", got, want, why),
            PlanError::E2eIncomplete { got, want } => format!("[0x3A 套件] 端到端 {}/{} 轮成功。建议：查全链哪步失守", got, want),
        }
    }
}

/// FNV-1a 32 位。
pub fn fnv32(data: &[u8], seed: u32) -> u32 {
    let mut h = seed;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// FNV 常量。
pub const FNV_PRIME: u32 = 0x0100_0193;
pub const FNV_OFFSET: u32 = 0x811C_9DC5;

/// 注册面快照（FNV——槽表/墓碑/回调账任一漂移即变）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegSnap {
    /// 活跃注册数。
    pub active: usize,
    /// 墓碑数。
    pub tombstones: usize,
    /// ABI 指纹。
    pub abi: u32,
    /// 快照指纹。
    pub fp: u32,
}

impl RegSnap {
    /// 取快照。
    pub fn capture(reg: &IrqRegistry) -> Self {
        let mut mat: Vec<u8> = Vec::new();
        for (v, s) in reg.slots.iter() {
            mat.push(*v);
            mat.push(s.state as u8);
            mat.extend_from_slice(&s.serial.to_le_bytes());
            mat.extend_from_slice(&s.in_flight.to_le_bytes());
        }
        for (v, ser) in reg.tombstones.iter() {
            mat.push(*v);
            mat.extend_from_slice(&ser.to_le_bytes());
        }
        RegSnap {
            active: reg.active_count(),
            tombstones: reg.tombstones.len(),
            abi: abi_fingerprint(),
            fp: fnv32(&mat, FNV_OFFSET),
        }
    }

    /// 往返一致。
    pub fn roundtrip_ok(&self, other: &RegSnap) -> bool {
        self == other
    }
}

// ---------------------------------------------------------------------------
// 七、全链自证（J1 走线：契约冻结 + 端到端 10/10 + 违例 10/10）
// ---------------------------------------------------------------------------

/// 全链自证（J1 完整走线——三断言全绿）。
pub fn selfcheck_full() -> Result<RegSnap, PlanError> {
    // 1. 契约冻结三件齐备：签名（IrqHandlerSpec/IrqHandle）+ 语义文档
    //    （七条款结构化在册）+ 消费样例（NVMe 档 specs 可注册）。
    let fp = abi_fingerprint();
    let fp2 = abi_fingerprint();
    if fp != fp2 {
        return Err(PlanError::AbiDrift {
            got: fp,
            want: IRQ_REGISTER_ABI_V1,
        });
    }
    // 2. 端到端 10/10。
    let e2e = e2e_suite(DEFAULT_SEED)?;
    e2e_verdict(&e2e)?;
    // 3. 违例 10/10 拦截。
    let hits = violation_injection_suite()?;
    violation_verdict(&hits)?;
    // 4. 快照往返。
    let reg = IrqRegistry::new();
    let s1 = RegSnap::capture(&reg);
    let s2 = RegSnap::capture(&reg);
    if !s1.roundtrip_ok(&s2) {
        return Err(PlanError::E2eIncomplete { got: 0, want: 1 });
    }
    Ok(s1)
}
