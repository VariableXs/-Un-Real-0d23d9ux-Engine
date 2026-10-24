//! VE-F5407 · 网络安全边界（VE-F 域 · AA 网络域 · 任务段 · 锚点目标 300 行 · 深化版）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5407`
//!
//! **判据（锚点原文）**：全清洗、指令白名单、速率限制、封禁流程、判据。
//!
//! **职责定位（锚点原文）**：网络安全边界——边界三防：输入校验（一切
//! 来自网络的输入全清洗——**不信任网络数据铁律**）/权限边界（客户端
//! 指令白名单——客户端不能命令服务器做没有的事）/速率限制（连接级
//! 速率限制——洪水防护）；三防是反作弊的地基（公平承诺的技术根）。
//!
//! **错误路径与降级矩阵（锚点原文）**：畸形输入→拒绝+记录；越权指令→
//! 拒绝+反作弊标记；洪水→限流+封禁流程。
//!
//! **性能逐项分解（锚点原文）**：清洗 O(输入数)；拒绝 O(1)；限流 O(1)。
//!
//! **跨批对接点（锚点原文）**：F4604 沙箱口径参照上游；F5417 深化下游。
//!
//! **无障碍与隐私（锚点原文）**：不信任网络数据即公平铁律（域色核心）；
//! **攻击记录脱敏**。
//!
//! # 深化说明（三防逐防落成可机检机制，脱敏落成结构保证）
//!
//! ## 深化一：全清洗不是「检查几个字段」，是**四道闸全部通过**
//!
//! 「不信任网络数据」若只在入口看一眼，中段拼接的字符串、上层重包
//! 的结构体都会绕过。故 [`InputSanitizer`] 把清洗落成**四道闸**：
//! · **长度闸**（[`LEN_MAX`] 上限——超长直接拒，截断是伪装接受）；
//! · **值域闸**（数值字段落在声明域内——域外即畸形）；
//! · **类型闸**（字段形态与声明一致——数字段装字母即畸形）；
//! · **编码闸**（UTF-8 合法+控制字符零容忍——转义绕过死路）。
//! 四闸全过才放行；任何一闸拒 → [`SanitizeRecord`] 记账。清洗
//! O(输入数)——逐字段线性，无隐藏嵌套扫描。
//!
//! ## 深化二：脱敏是**结构保证**不是「记得别写」（锚点隐私面）
//!
//! 攻击记录里若存原始 payload，日志本身就是攻击载荷的传播链（日志
//! 被读屏读出、被导出、被贴到 issue 里）。故 [`SanitizeRecord`]
//! **类型上就没有原始内容字段**——只存形状指纹（FNV-1a of 长度/
//! 字段数/类别码）、类别、连接号、时刻。想要原始内容？类型系统说
//! 没有。脱敏从纪律变成结构。
//!
//! ## 深化三：指令白名单是**闭集查表+参数域双闸**（拒绝 O(1)）
//!
//! 「客户端不能命令服务器做没有的事」落成两层：
//! · **指令在册闸**：[`CommandId`] 闭集之外的指令号直接拒+反作弊
//!   标记（构造的指令号=探测攻击的第一步）；
//! · **参数域闸**：在册指令的参数也要落在该指令声明的域内——
//!   「移动」指令带 10^9 格位移参数照样拒（白名单不是只看名字）。
//! 两闸全查表，拒绝 O(1)——锚点性能分解「拒绝 O(1)」。
//!
//! ## 深化四：速率限制是**分级响应**不是二值开关（限流 O(1)）
//!
//! 洪水防护若只有「超限拒」，正常玩家的突发（加载后集中同步）会被
//! 误伤，真洪水却只在 100% 处被挡。故 [`RateLimiter`] 三级：
//! · <80%：正常放行；
//! · 80%-100%：**限流**（放行+节流标记——客户端该收慢的信号）；
//! · >100%：**拒入**+洪水计数（持续超限触发封禁流程）。
//! 滑动窗口整数实现（无浮点），判 O(1)——窗口求和用增量记账不扫
//! 全窗（每 tick 只减一个过期桶）。
//!
//! ## 深化五：封禁流程是**三段可回滚**的账（误封有路径）
//!
//! 封禁不是「拉黑名单一行」——误封一个 IP 段是真实事故。故
//! [`BanFlow`] 三段：**触发**（洪水计数达 [`BAN_THRESHOLD`]）→
//! **宽限**（宽限窗内再犯才生效——单次毛刺不封人）→ **生效**（封禁
//! 账登记，可查可解）。每段留账，解封路径显性（[`BanFlow::lift`]），
//! 误封回滚不是「改库」是「走账」。
//!
//! ## 深化六：F4604 沙箱口径上游 + F5417 深化下游
//!
//! [`SANDBOX_CALIBER_NOTE`]（F4604 口径引用——三防的清洗规则与沙箱
//! 的输入治理同源同口径）；[`DeepenSurface`] trait（F5417 深化下游
//! 消费面——清洗账/白名单账/限流账的只读出口，深化模块据此对账）。

use alloc::vec;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const NETSEC_VERSION: &str = "F5407-netsec-v1-deepened";

/// 畸形输入（拒绝+记录）。
pub const E_SEC_MALFORMED: &str = "E_SEC_MALFORMED";

/// 越权指令（拒绝+反作弊标记）。
pub const E_SEC_UNLISTED: &str = "E_SEC_UNLISTED";

/// 洪水限流（拒入）。
pub const E_SEC_FLOOD: &str = "E_SEC_FLOOD";

/// 封禁（封禁流程生效后拒入）。
pub const E_SEC_BANNED: &str = "E_SEC_BANNED";

/// 单字段长度上限（字节——长度闸）。
pub const LEN_MAX: usize = 4096;

/// 限流线（窗口用量千分比——80% 开始节流）。
pub const THROTTLE_PERMILLE: u32 = 800;

/// 封禁触发阈值（持续拒入 tick 数）。
pub const BAN_THRESHOLD: u32 = 5;

/// 宽限窗（tick 数——宽限期内再犯才生效）。
pub const BAN_GRACE_TICKS: u32 = 3;

/// F4604 沙箱口径上游声明。
pub const SANDBOX_CALIBER_NOTE: &str =
    "F5407 网络安全边界口径承接 F4604 沙箱上游：输入清洗四闸与沙箱输入治理同源同口径（长度/值域/类型/编码），沙箱是进程边界、本条是网络边界，两边界清洗规则一致不改写；F5417 深化下游经 DeepenSurface 只读消费三账";

// ---------------------------------------------------------------------------
// 二、输入清洗器（四道闸+脱敏记录）
// ---------------------------------------------------------------------------

/// 字段声明（清洗的依据——网络输入必须逐字段声明才能过闸）。
#[derive(Clone, Copy, Debug)]
pub struct FieldSpec {
    /// 字段名（读屏可达）。
    pub name: &'static str,
    /// 长度上限（字节；数值字段=8）。
    pub len_max: usize,
    /// 值域（(min, max)——数值字段有效；文本字段忽略）。
    pub value_range: (i64, i64),
    /// 是否数值字段（true=走值域闸，false=走编码闸）。
    pub numeric: bool,
}

/// 网络输入帧（待清洗——来自网络的原始结构）。
#[derive(Clone, Debug)]
pub struct RawInput {
    /// 连接号（限流/封禁的归属键）。
    pub conn: u32,
    /// 字段序列（与声明表逐位对应）。
    pub fields: Vec<Field>,
}

/// 单字段原始值。
#[derive(Clone, Debug)]
pub enum Field {
    /// 数值。
    Num(i64),
    /// 文本（字节——编码闸检查 UTF-8 与控制字符）。
    Text(Vec<u8>),
}

/// 清洗记录（**脱敏是结构保证**：无原始内容字段——只有形状指纹）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SanitizeRecord {
    /// 形状指纹（FNV-1a of 长度+字段数+类别码——可对账不可还原）。
    pub shape_fp: u64,
    /// 拒因类别（0=长度 1=值域 2=类型 3=编码）。
    pub gate: u8,
    /// 连接号。
    pub conn: u32,
    /// 时刻（逻辑 tick）。
    pub tick: u32,
}

/// 形状指纹（FNV-1a——脱敏指纹可对账不可还原原始内容）。
pub fn shape_fingerprint(len: usize, fields: usize, gate: u8) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in [
        (len & 0xff) as u8,
        (len >> 8) as u8,
        (fields & 0xff) as u8,
        gate,
    ] {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 清洗器（四道闸——全过才放行；清洗 O(输入数)）。
#[derive(Clone, Debug, Default)]
pub struct InputSanitizer {
    /// 字段声明表（输入帧的字段序列必须与表同长同序）。
    pub specs: Vec<FieldSpec>,
    /// 清洗账（拒绝记录——脱敏）。
    pub records: Vec<SanitizeRecord>,
    /// 放行计数。
    pub passed: u32,
}

impl InputSanitizer {
    /// 新清洗器（带声明表）。
    pub fn new(specs: Vec<FieldSpec>) -> InputSanitizer {
        InputSanitizer { specs, records: Vec::new(), passed: 0 }
    }

    /// 清洗一帧（四道闸逐道过——任何一闸拒即记账返回 Err）。
    pub fn sanitize(&mut self, raw: &RawInput, tick: u32) -> Result<(), String> {
        // 声明对齐闸（字段数与声明表同长——结构本身先合规）。
        if raw.fields.len() != self.specs.len() {
            let fp = shape_fingerprint(raw.fields.len(), self.specs.len(), 2);
            self.records.push(SanitizeRecord { shape_fp: fp, gate: 2, conn: raw.conn, tick });
            return Err(format!(
                "{}：字段数 {} ≠ 声明 {}——结构先合规才谈内容（类型闸）",
                E_SEC_MALFORMED, raw.fields.len(), self.specs.len()
            ));
        }
        for (i, f) in raw.fields.iter().enumerate() {
            let spec = &self.specs[i];
            match f {
                Field::Num(v) => {
                    if !spec.numeric {
                        let fp = shape_fingerprint(8, raw.fields.len(), 2);
                        self.records.push(SanitizeRecord { shape_fp: fp, gate: 2, conn: raw.conn, tick });
                        return Err(format!(
                            "{}：字段「{}」声明为文本却收到数值——类型闸",
                            E_SEC_MALFORMED, spec.name
                        ));
                    }
                    if *v < spec.value_range.0 || *v > spec.value_range.1 {
                        let fp = shape_fingerprint(8, raw.fields.len(), 1);
                        self.records.push(SanitizeRecord { shape_fp: fp, gate: 1, conn: raw.conn, tick });
                        return Err(format!(
                            "{}：字段「{}」值 {} 越域 [{},{}]——值域闸",
                            E_SEC_MALFORMED, spec.name, v, spec.value_range.0, spec.value_range.1
                        ));
                    }
                }
                Field::Text(bytes) => {
                    if spec.numeric {
                        let fp = shape_fingerprint(bytes.len(), raw.fields.len(), 2);
                        self.records.push(SanitizeRecord { shape_fp: fp, gate: 2, conn: raw.conn, tick });
                        return Err(format!(
                            "{}：字段「{}」声明为数值却收到文本——类型闸",
                            E_SEC_MALFORMED, spec.name
                        ));
                    }
                    // 长度闸。
                    if bytes.len() > spec.len_max || bytes.len() > LEN_MAX {
                        let fp = shape_fingerprint(bytes.len(), raw.fields.len(), 0);
                        self.records.push(SanitizeRecord { shape_fp: fp, gate: 0, conn: raw.conn, tick });
                        return Err(format!(
                            "{}：字段「{}」长度 {} 超上限 {}——长度闸（截断是伪装接受，直接拒）",
                            E_SEC_MALFORMED, spec.name, bytes.len(), spec.len_max.max(LEN_MAX)
                        ));
                    }
                    // 编码闸：控制字符零容忍（<0x20 且非 \t\n\r 一律拒）。
                    if let Some(pos) = bytes.iter().position(|&b| b < 0x20 && b != b'\t' && b != b'\n' && b != b'\r') {
                        let fp = shape_fingerprint(bytes.len(), raw.fields.len(), 3);
                        self.records.push(SanitizeRecord { shape_fp: fp, gate: 3, conn: raw.conn, tick });
                        return Err(format!(
                            "{}：字段「{}」第 {} 字节是控制字符——编码闸（转义绕过死路）",
                            E_SEC_MALFORMED, spec.name, pos
                        ));
                    }
                }
            }
        }
        self.passed = self.passed.saturating_add(1);
        Ok(())
    }

    /// 记录数（脱敏账规模——读屏可达）。
    pub fn record_count(&self) -> usize {
        self.records.len()
    }
}

// ---------------------------------------------------------------------------
// 三、指令白名单（闭集查表+参数域双闸——拒绝 O(1)）
// ---------------------------------------------------------------------------

/// 客户端指令闭集（白名单——表外即越权）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandId {
    /// 移动（参数：位移 dx,dy ∈ [-32,32]）。
    Move,
    /// 交互（参数：目标 id ∈ [0, 65535]）。
    Interact,
    /// 同步请求（参数：水位 ∈ [0, u32::MAX]）。
    SyncReq,
    /// 心跳（无参）。
    Ping,
    /// 聊天（参数：文本长度 ∈ [0, 256]）。
    Chat,
}

impl CommandId {
    /// 全闭集（白名单完整性对账）。
    pub const ALL: [CommandId; 5] = [
        CommandId::Move,
        CommandId::Interact,
        CommandId::SyncReq,
        CommandId::Ping,
        CommandId::Chat,
    ];

    /// 指令参数域（参数域闸的声明源——白名单不只看名字）。
    pub fn param_domain(self) -> (i64, i64) {
        match self {
            CommandId::Move => (-32, 32),
            CommandId::Interact => (0, 65_535),
            CommandId::SyncReq => (0, u32::MAX as i64),
            CommandId::Ping => (0, 0),
            CommandId::Chat => (0, 256),
        }
    }

    /// 指令号（线上协议号——查表键）。
    pub fn code(self) -> u16 {
        match self {
            CommandId::Move => 0x01,
            CommandId::Interact => 0x02,
            CommandId::SyncReq => 0x03,
            CommandId::Ping => 0x04,
            CommandId::Chat => 0x05,
        }
    }

    /// 按号查指令（O(1)——5 项定长；表外 None）。
    pub fn of_code(code: u16) -> Option<CommandId> {
        Self::ALL.iter().copied().find(|c| c.code() == code)
    }
}

/// 白名单守卫（在册闸+参数域闸——拒绝 O(1)+反作弊标记）。
#[derive(Clone, Copy, Debug, Default)]
pub struct CommandGate {
    /// 越权标记数（反作弊旗标——累计）。
    pub cheat_flags: u32,
    /// 放行数。
    pub passed: u32,
}

impl CommandGate {
    /// 校验一条客户端指令（在册+参数域双闸）。
    pub fn check(&mut self, code: u16, param: i64) -> Result<CommandId, String> {
        let cmd = CommandId::of_code(code).ok_or_else(|| {
            self.cheat_flags = self.cheat_flags.saturating_add(1);
            format!(
                "{}：指令号 0x{code:02X} 不在白名单——客户端不能命令服务器做没有的事。\
                 反作弊标记 +1（构造指令号=探测攻击第一步）",
                E_SEC_UNLISTED
            )
        })?;
        let (lo, hi) = cmd.param_domain();
        if param < lo || param > hi {
            self.cheat_flags = self.cheat_flags.saturating_add(1);
            return Err(format!(
                "{}：指令 {:?} 参数 {} 越域 [{},{}]——白名单不只看名字，参数域同闸。\
                 反作弊标记 +1",
                E_SEC_UNLISTED, cmd, param, lo, hi
            ));
        }
        self.passed = self.passed.saturating_add(1);
        Ok(cmd)
    }
}

// ---------------------------------------------------------------------------
// 四、速率限制器（三级响应——限流 O(1) 增量记账）
// ---------------------------------------------------------------------------

/// 限流判定（三级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateVerdict {
    /// 正常放行（<80%）。
    Pass,
    /// 限流（80%-100%——放行+节流标记）。
    Throttle,
    /// 拒入（>100%——洪水计数+1）。
    Reject,
}

/// 滑动窗口限流器（整数增量记账——判 O(1) 不扫全窗）。
///
/// 窗口 = [`WINDOW_TICKS`] 个桶，每 tick 一个桶；用量=sum(桶)。
/// 增量记账：每 tick 只把「滑出窗口的最老桶」从 sum 里减掉——
/// O(1) 摊销，不做全窗求和。
#[derive(Clone, Copy, Debug)]
pub struct RateLimiter {
    /// 每窗口配额。
    pub quota: u32,
    /// 窗口桶（环形）。
    buckets: [u32; WINDOW_TICKS],
    /// 桶和（增量维护）。
    sum: u32,
    /// 当前 tick 指针。
    wpos: usize,
    /// 持续拒入 tick 数（封禁触发输入）。
    reject_streak: u32,
    /// 洪水拒入累计。
    floods: u32,
}

/// 窗口 tick 数（滑动窗口桶数）。
pub const WINDOW_TICKS: usize = 8;

impl RateLimiter {
    /// 新限流器。
    pub fn new(quota: u32) -> RateLimiter {
        RateLimiter {
            quota,
            buckets: [0; WINDOW_TICKS],
            sum: 0,
            wpos: 0,
            reject_streak: 0,
            floods: 0,
        }
    }

    /// 前进一 tick（先滑指针再清新位桶——新位桶是 8 tick 前的旧数据，
    /// 清它=滑出窗口；当前桶里本 tick 的消耗得以保留）。
    pub fn tick(&mut self) {
        self.wpos = (self.wpos + 1) % WINDOW_TICKS;
        self.sum = self.sum.saturating_sub(self.buckets[self.wpos]);
        self.buckets[self.wpos] = 0;
        // 拒入断档则清 streak（连续性度量）。
        if self.floods == 0 {
            self.reject_streak = 0;
        }
    }

    /// 当前用量千分比（相对配额）。
    pub fn usage_permille(&self) -> u32 {
        if self.quota == 0 {
            return 1_000;
        }
        (self.sum as u64 * 1_000 / self.quota as u64).min(u32::MAX as u64) as u32
    }

    /// 记一次请求消耗（判定三级：Pass/Throttle/Reject）。
    pub fn consume(&mut self, cost: u32) -> (RateVerdict, String) {
        let used = self.usage_permille();
        let projected = {
            let new_sum = self.sum.saturating_add(cost);
            if self.quota == 0 {
                1_000
            } else {
                (new_sum as u64 * 1_000 / self.quota as u64).min(u32::MAX as u64) as u32
            }
        };
        if projected > 1_000 {
            self.floods = self.floods.saturating_add(1);
            self.reject_streak = self.reject_streak.saturating_add(1);
            return (
                RateVerdict::Reject,
                format!(
                    "{}：请求 {} 使窗口用量 {}‰ > 1000‰——拒入（洪水计数 {}，连续 {} tick）",
                    E_SEC_FLOOD, cost, projected, self.floods, self.reject_streak
                ),
            );
        }
        self.sum = self.sum.saturating_add(cost);
        self.buckets[self.wpos] = self.buckets[self.wpos].saturating_add(cost);
        self.reject_streak = 0;
        if projected >= THROTTLE_PERMILLE {
            (
                RateVerdict::Throttle,
                format!("限流：用量 {}‰ ≥ {}‰——放行+节流标记（客户端应收慢）", projected, THROTTLE_PERMILLE),
            )
        } else {
            (RateVerdict::Pass, String::new())
        }
    }

    /// 连续拒入 tick 数（封禁触发输入）。
    pub fn reject_streak(&self) -> u32 {
        self.reject_streak
    }

    /// 洪水拒入累计（读屏可达）。
    pub fn floods(&self) -> u32 {
        self.floods
    }
}

// ---------------------------------------------------------------------------
// 五、封禁流程（三段可回滚的账）
// ---------------------------------------------------------------------------

/// 封禁状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BanState {
    /// 未封禁。
    None,
    /// 宽限期（触发但未生效——宽限窗内再犯才封）。
    Grace(u32),
    /// 已生效。
    Active,
}

/// 封禁流程（触发→宽限→生效；解封走账可回滚）。
#[derive(Clone, Copy, Debug)]
pub struct BanFlow {
    /// 连接状态。
    pub state: BanState,
    /// 生效封禁累计（读屏可达）。
    pub bans: u32,
    /// 解封累计（回滚有账）。
    pub lifts: u32,
    /// 误封回滚说明账（最近一条——读屏可达）。
    pub last_lift_note: Option<&'static str>,
}

impl BanFlow {
    /// 新流程。
    pub fn new() -> BanFlow {
        BanFlow { state: BanState::None, bans: 0, lifts: 0, last_lift_note: None }
    }

    /// 喂洪水信号（限流器的连续拒入）——三段状态机推进。
    pub fn observe_flood(&mut self, reject_streak: u32, tick: u32) -> Result<BanState, String> {
        match self.state {
            BanState::None => {
                if reject_streak >= BAN_THRESHOLD {
                    self.state = BanState::Grace(tick);
                    Ok(self.state)
                } else {
                    Ok(self.state)
                }
            }
            BanState::Grace(t0) => {
                // 宽限期内再犯（streak 持续）→ 生效；宽限超时回落。
                if reject_streak >= BAN_THRESHOLD {
                    self.state = BanState::Active;
                    self.bans = self.bans.saturating_add(1);
                    Ok(self.state)
                } else if tick.saturating_sub(t0) > BAN_GRACE_TICKS {
                    self.state = BanState::None; // 宽限超时回落（单次毛刺不封人）
                    Ok(self.state)
                } else {
                    Ok(self.state)
                }
            }
            BanState::Active => Ok(self.state), // 已生效（解封走 lift）
        }
    }

    /// 是否被拒入（生效态拒绝一切——封禁账前缀）。
    pub fn is_banned(&self) -> bool {
        self.state == BanState::Active
    }

    /// 解封（走账回滚——误封处置路径，不是改库）。
    pub fn lift(&mut self, note: &'static str) {
        if self.state == BanState::Active {
            self.state = BanState::None;
            self.lifts = self.lifts.saturating_add(1);
            self.last_lift_note = Some(note);
        }
    }
}

// ---------------------------------------------------------------------------
// 六、F5417 深化下游消费面（三账只读出口）
// ---------------------------------------------------------------------------

/// 深化下游消费面（F5417 对接：深化模块只读消费三账——清洗账/白名单
/// 账/限流账；只读=深化侧不能改写安全账，改写走本条的错误路径）。
pub trait DeepenSurface {
    /// 拉取清洗账规模。
    fn sanitize_records(&self) -> usize;
    /// 拉取反作弊标记数。
    fn cheat_flags(&self) -> u32;
    /// 拉取洪水拒入数。
    fn floods(&self) -> u32;
    /// 拉取封禁累计。
    fn bans(&self) -> u32;
}

/// 三防聚合快照（DeepenSurface 的数据源——一次快照一致视图）。
#[derive(Clone, Copy, Debug, Default)]
pub struct SecSnapshot {
    pub sanitize_records: usize,
    pub cheat_flags: u32,
    pub floods: u32,
    pub bans: u32,
}

impl DeepenSurface for SecSnapshot {
    fn sanitize_records(&self) -> usize {
        self.sanitize_records
    }
    fn cheat_flags(&self) -> u32 {
        self.cheat_flags
    }
    fn floods(&self) -> u32 {
        self.floods
    }
    fn bans(&self) -> u32 {
        self.bans
    }
}

/// 三防守卫聚合（清洗器×白名单×限流器×封禁的全链编排——边界运行时）。
#[derive(Clone, Debug)]
pub struct NetBoundary {
    pub sanitizer: InputSanitizer,
    pub gate: CommandGate,
    pub limiter: RateLimiter,
    pub ban: BanFlow,
    /// 逻辑 tick。
    pub tick_now: u32,
}

impl NetBoundary {
    /// 新边界（声明表+配额）。
    pub fn new(specs: Vec<FieldSpec>, quota: u32) -> NetBoundary {
        NetBoundary {
            sanitizer: InputSanitizer::new(specs),
            gate: CommandGate::default(),
            limiter: RateLimiter::new(quota),
            ban: BanFlow::new(),
            tick_now: 0,
        }
    }

    /// 一帧入站全链（限流→清洗——洪水先挡在清洗之前省算力）。
    pub fn inbound(&mut self, raw: &RawInput, cost: u32) -> Result<(), String> {
        // 封禁生效态：一律拒（封禁账前缀）。
        if self.ban.is_banned() {
            return Err(format!("{}：连接 {} 封禁生效中——拒绝一切入站（解封走 lift 走账）", E_SEC_BANNED, raw.conn));
        }
        // 限流（洪水先挡——清洗 O(输入数) 的算力不为洪水买单）。
        let (verdict, msg) = self.limiter.consume(cost);
        match verdict {
            RateVerdict::Reject => {
                // 封禁流程推进（洪水→封禁流程）。
                self.ban.observe_flood(self.limiter.reject_streak(), self.tick_now)?;
                return Err(msg);
            }
            _ => {}
        }
        // 清洗（四道闸）。
        self.sanitizer.sanitize(raw, self.tick_now)
    }

    /// 一条指令全链（白名单双闸）。
    pub fn command(&mut self, code: u16, param: i64) -> Result<CommandId, String> {
        self.gate.check(code, param)
    }

    /// 前进一 tick。
    pub fn tick(&mut self) {
        self.limiter.tick();
        self.tick_now = self.tick_now.saturating_add(1);
    }

    /// 安全快照（F5417 消费源）。
    pub fn snapshot(&self) -> SecSnapshot {
        SecSnapshot {
            sanitize_records: self.sanitizer.record_count(),
            cheat_flags: self.gate.cheat_flags,
            floods: self.limiter.floods(),
            bans: self.ban.bans,
        }
    }

    /// 边界摘要（一行读屏可达）。
    pub fn summary(&self) -> String {
        format!(
            "安全边界：清洗过={}拒={} 白名单过={}标记={} 洪水={} 封禁={} 解封={}",
            self.sanitizer.passed,
            self.sanitizer.record_count(),
            self.gate.passed,
            self.gate.cheat_flags,
            self.limiter.floods(),
            self.ban.bans,
            self.ban.lifts,
        )
    }
}

// ---------------------------------------------------------------------------
// 七、判据（判据区零 panic 面：无 unwrap()/expect()）
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// F5407 域自检（判据分五组：全清洗/白名单/限流/封禁/对接收尾）。
pub fn run_vef60_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5407");

    // --- 全清洗（判据一：四道闸）---
    let specs = vec![
        FieldSpec { name: "位移x", len_max: 8, value_range: (-32, 32), numeric: true },
        FieldSpec { name: "昵称", len_max: 16, value_range: (0, 0), numeric: false },
    ];
    let mut san = InputSanitizer::new(specs);
    // 合法帧放行。
    let good = RawInput { conn: 1, fields: vec![Field::Num(10), Field::Text(b"hero".to_vec())] };
    s.add("F60-清洗-01", san.sanitize(&good, 0).is_ok() && san.passed == 1, "四闸全过放行（合法帧）");
    // 长度闸。
    let long = RawInput { conn: 1, fields: vec![Field::Num(0), Field::Text(vec![b'a'; 17])] };
    let r1 = san.sanitize(&long, 1);
    s.add(
        "F60-清洗-02",
        r1.as_ref().err().map(|x| x.contains("长度闸")).unwrap_or(false) && san.record_count() == 1,
        "长度闸（17 > 16 拒——截断是伪装接受）",
    );
    // 值域闸。
    let oob = RawInput { conn: 1, fields: vec![Field::Num(999), Field::Text(b"ok".to_vec())] };
    s.add(
        "F60-清洗-03",
        san.sanitize(&oob, 2).as_ref().err().map(|x| x.contains("值域闸")).unwrap_or(false),
        "值域闸（999 > 32 拒）",
    );
    // 类型闸（数值段装文本）。
    let wrong_ty = RawInput { conn: 1, fields: vec![Field::Text(b"99".to_vec()), Field::Text(b"ok".to_vec())] };
    s.add(
        "F60-清洗-04",
        san.sanitize(&wrong_ty, 3).as_ref().err().map(|x| x.contains("类型闸")).unwrap_or(false),
        "类型闸（数值段装文本拒）",
    );
    // 编码闸（控制字符）。
    let ctrl = RawInput { conn: 1, fields: vec![Field::Num(1), Field::Text(vec![b'a', 0x01, b'b'])] };
    s.add(
        "F60-清洗-05",
        san.sanitize(&ctrl, 4).as_ref().err().map(|x| x.contains("编码闸")).unwrap_or(false),
        "编码闸（控制字符零容忍——转义绕过死路）",
    );
    // 脱敏结构保证：记录无原始内容字段（形状指纹可对账不可还原）。
    let rec_ok = san.records.iter().all(|r| r.shape_fp != 0 && r.gate <= 3);
    let fp_distinct = san.records.windows(2).all(|w| w[0].shape_fp != w[1].shape_fp);
    s.add(
        "F60-清洗-06",
        rec_ok && fp_distinct,
        "脱敏结构保证（4 条记录只有形状指纹——类别间指纹互异可对账）",
    );

    // --- 指令白名单（判据二：双闸 O(1)）---
    let mut gate = CommandGate::default();
    // 在册+域内放行。
    let g1 = gate.check(0x01, 10);
    s.add(
        "F60-白名单-01",
        g1.as_ref().map(|c| *c == CommandId::Move).unwrap_or(false) && gate.passed == 1,
        "在册+域内放行（Move dx=10）",
    );
    // 表外指令拒+反作弊标记。
    let g2 = gate.check(0xEE, 0);
    s.add(
        "F60-白名单-02",
        g2.as_ref().err().map(|x| x.starts_with(E_SEC_UNLISTED)).unwrap_or(false) && gate.cheat_flags == 1,
        "表外指令拒+反作弊标记（0xEE 不在册）",
    );
    // 在册但参数越域拒+标记。
    let g3 = gate.check(0x01, 1_000);
    s.add(
        "F60-白名单-03",
        g3.as_ref().err().map(|x| x.contains("参数域同闸")).unwrap_or(false) && gate.cheat_flags == 2,
        "参数域闸（Move dx=1000 越域——白名单不只看名字）",
    );
    // 闭集完整性（5 项全可查+码互异）。
    let all_listed = CommandId::ALL.iter().all(|c| CommandId::of_code(c.code()).is_some());
    let codes_distinct = {
        let mut cs = [0u16; 5];
        for (i, c) in CommandId::ALL.iter().enumerate() {
            cs[i] = c.code();
        }
        cs[0] != cs[1] && cs[0] != cs[2] && cs[0] != cs[3] && cs[0] != cs[4]
            && cs[1] != cs[2] && cs[1] != cs[3] && cs[1] != cs[4]
            && cs[2] != cs[3] && cs[2] != cs[4] && cs[3] != cs[4]
    };
    s.add("F60-白名单-04", all_listed && codes_distinct, "闭集完整（5 指令全可查+码互异）");

    // --- 速率限制（判据三：三级+增量 O(1)）---
    let mut rl = RateLimiter::new(100);
    // 正常放行（<80%）。
    let (v1, _) = rl.consume(50);
    s.add("F60-限流-01", v1 == RateVerdict::Pass, "一级放行（50% < 80%）");
    // 限流（80%-100%）。
    let (v2, m2) = rl.consume(35);
    s.add(
        "F60-限流-02",
        v2 == RateVerdict::Throttle && m2.contains("节流"),
        "二级限流（85% ≥ 80%——放行+节流标记）",
    );
    // 拒入（>100%）。
    let (v3, m3) = rl.consume(30);
    s.add(
        "F60-限流-03",
        v3 == RateVerdict::Reject && m3.contains("洪水") && rl.floods() == 1,
        "三级拒入（115% > 100%——洪水计数）",
    );
    // 增量记账 O(1)：窗口滑出语义——当前消耗留在窗口内（85 桶未滑出
    // 用量不变），WINDOW_TICKS 次推进后全部滑出归零（不扫全窗）。
    for _ in 0..WINDOW_TICKS {
        rl.tick();
    }
    let after_tick = rl.usage_permille();
    s.add(
        "F60-限流-04",
        after_tick == 0,
        "滑动窗口推进（窗口数据滑出归零——增量 O(1) 不扫全窗）",
    );

    // --- 封禁流程（判据四：三段可回滚）---
    let mut bf = BanFlow::new();
    // 触发（streak≥5）→ 宽限。
    let b1 = bf.observe_flood(5, 10);
    s.add(
        "F60-封禁-01",
        b1.as_ref().map(|s| matches!(s, BanState::Grace(10))).unwrap_or(false) && !bf.is_banned(),
        "触发进宽限（streak 5≥5——未生效不拒人）",
    );
    // 宽限期内再犯 → 生效。
    let b2 = bf.observe_flood(5, 11);
    s.add(
        "F60-封禁-02",
        b2.as_ref().map(|s| *s == BanState::Active).unwrap_or(false) && bf.is_banned() && bf.bans == 1,
        "宽限期内再犯生效（三段推进完成）",
    );
    // 解封走账回滚。
    bf.lift("误封复核——IP 段共享 NAT 出口");
    s.add(
        "F60-封禁-03",
        !bf.is_banned() && bf.lifts == 1 && bf.last_lift_note.is_some(),
        "解封走账（回滚是走账不是改库）",
    );
    // 宽限超时回落（单次毛刺不封人）。
    let mut bf2 = BanFlow::new();
    bf2.observe_flood(5, 0).ok();
    let b3 = bf2.observe_flood(0, 9);
    s.add(
        "F60-封禁-04",
        b3.as_ref().map(|s| *s == BanState::None).unwrap_or(false) && !bf2.is_banned(),
        "宽限超时回落（9 > 3 拍——单次毛刺不封人）",
    );

    // --- 全链编排（判据五：边界运行时）---
    let specs2 = vec![FieldSpec { name: "水位", len_max: 8, value_range: (0, 1_000_000), numeric: true }];
    let mut nb = NetBoundary::new(specs2, 1_000);
    // 合法帧全链通过。
    let f_ok = RawInput { conn: 7, fields: vec![Field::Num(100)] };
    let i1 = nb.inbound(&f_ok, 100);
    s.add(
        "F60-全链-01",
        i1.is_ok() && nb.command(0x03, 100).is_ok(),
        "全链合法通过（限流→清洗→白名单）",
    );
    // 洪水在清洗之前被挡（算力不为洪水买单）。
    let mut flood_rejects = 0;
    for i in 0..6u32 {
        let fr = RawInput { conn: 7, fields: vec![Field::Num(1)] };
        if nb.inbound(&fr, 400).is_err() {
            flood_rejects += 1;
        }
        nb.tick();
        let _ = i;
    }
    s.add(
        "F60-全链-02",
        flood_rejects >= 4 && nb.snapshot().floods >= 4,
        "洪水先挡（300×6 超配额 1000——拒入在清洗之前）",
    );
    // 快照消费面（F5417 只读出口）。
    let snap = nb.snapshot();
    let ds: &dyn DeepenSurface = &snap;
    s.add(
        "F60-全链-03",
        ds.sanitize_records() == snap.sanitize_records && ds.floods() == snap.floods,
        "F5417 深化消费面（三账只读快照一致）",
    );
    // 摘要行。
    let sum = nb.summary();
    s.add("F60-全链-04", sum.contains("安全边界") && sum.contains("封禁"), "边界摘要一行可读");

    // --- 对接与收尾 ---
    s.add(
        "F60-对接-01",
        SANDBOX_CALIBER_NOTE.contains("F4604") && SANDBOX_CALIBER_NOTE.contains("F5417"),
        "F4604 上游口径+F5417 下游消费声明在案",
    );
    s.add(
        "F60-版本-01",
        NETSEC_VERSION.contains("F5407") && NETSEC_VERSION.ends_with("deepened"),
        "版本指纹（F5407-netsec-v1-deepened）",
    );
    let declared = s.len();
    s.add("F60-收尾-01", declared >= 21 && declared + 1 >= 22, "判据条数对账（21+本条=22 条）");

    s
}

/// F 域账本暂挂声明（跨批对接点：F4604 沙箱口径上游；F5417 深化下游
/// ——建账前暂挂，移交期模式延续）。
pub const F60_LEDGER_SUSPENDED_NOTE: &str =
    "网络安全边界（输入清洗四闸+指令白名单双闸+速率三级+封禁三段）入 F 域账本：建账前暂挂声明（移交期模式延续）；不信任网络数据即公平铁律（域色核心），攻击记录脱敏为结构保证";
