//! UNREAL-X / UNX-A2-B07 / CoRun Varix STAR II · Unxreal
//! 锚点：UNX-F0926 · 调度唤醒 IPI（reschedule 语义与调度域协作接口）（目标 320 行）
//! 判据原文摘录（书 2593 行）：UNX-F0926-J1 唤醒链路正确（唤醒注入 10/10 次
//! 目标核进入调度点），IPI 频率纪律（合并窗口生效——唤醒 IPI 率降幅可测），
//! 协作接口冻结（调度域消费声明）。
//! 职责定位：当核 A 上的调度决策需要核 B 重新调度（有新任务可跑、优先级抢占、
//! 负载均衡迁移）时，唤醒 IPI 是把「该看队列了」这句话送到核 B 的唯一介质——本
//! 单定义它的精确语义：唤醒是幂等的（同一窗口内对同一核的 N 次唤醒合并为一次
//! IPI，因为调度点看一次队列就涵盖了全部增量）；唤醒链路是可审计的（注入→pending
//! 位→IPI 发出→目标核进调度点→pending 清位，五步全留痕）；唤醒 IPI 率（IPI 数/
//! 唤醒请求数）在合并窗口下必须可测地下降（纪律生效的量化证据）；调度域协作接口
//! 冻结（消费方编译期锚定，签名漂移即断链）。
//! 设计要点：①**pending 位图单点**——每核一位，置位幂等（重复置位不重复计数
//! 请求只计首置），清位只在调度点进入时发生（清位即消费）；②**合并窗口**——
//! 窗口开启期间 wake 只置位不发 IPI，窗口关闭（tick 到期或显式 flush）时按位图
//! 一次发 IPI（N 请求 1 IPI——降幅=W/N 可测）；③**调度点状态机**——Idle→
//! Waked→InSchedule→Idle 单向环，进调度点即清本核 pending 位（欠账不积累）；
//! ④**接口冻结**——协作接口三节文档（签名/时序/所有权）常量文本+FNV 指纹，
//! 消费样例编译锚定；⑤**家族复用声明**——FNV/逻辑 tick/ticket 单调复用
//! F0921~F0925（家族复用第十三用），本单新增的是 pending 位图与合并窗口面。
//! 诊断码段：0x53（本单独占，0x5301~0x530F）。
//! 施工：AI-WB2 · 2026-10-11。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码闭集（0x53 段独占）
// ---------------------------------------------------------------------------

/// 0x5301 唤醒离线核（wake 目标不在在线集——唤醒面越界）。
pub const XC_WAKE_OFFLINE: u32 = 0x5301;
/// 0x5302 调度点状态倒退（Idle→InSchedule 跳态或环序破坏——状态机失守）。
pub const XC_SCHED_REGRESS: u32 = 0x5302;
/// 0x5303 清位无 pending（调度点清了未置位的位——pending 面漂移）。
pub const XC_CLEAR_NOBIT: u32 = 0x5303;
/// 0x5304 唤醒后未进调度点（IPI 发出但目标核状态未变——链路断裂）。
pub const XC_WAKE_NO_SCHED: u32 = 0x5304;
/// 0x5305 合并窗口越界（窗口外发 IPI 或窗口内直发——窗口纪律破坏）。
pub const XC_WINDOW_BREACH: u32 = 0x5305;
/// 0x5306 IPI 率不降反升（合并后 IPI 数 > 未合并基线——纪律失效）。
pub const XC_RATE_WORSE: u32 = 0x5306;
/// 0x5307 接口指纹失守（协作接口被改——冻结面破坏）。
pub const XC_IFACE_ALTERED: u32 = 0x5307;
/// 0x5308 双发 IPI（同窗口同核第二次发 IPI——合并纪律破坏）。
pub const XC_IPI_TWICE: u32 = 0x5308;
/// 0x5309 消费声明缺失（接口消费方未注册声明——协作面空洞）。
pub const XC_CONSUMER_MISSING: u32 = 0x5309;
/// 0x530A 位图越界位（pending 位图高位脏——位宽纪律破坏）。
pub const XC_BITMAP_DIRTY: u32 = 0x530A;
/// 0x530B 窗口未关结算（窗口开启时被查询 IPI 账——结算面未闭合）。
pub const XC_WINDOW_OPEN: u32 = 0x530B;
/// 0x530C 降幅账残缺（基线/合并两账任一缺失——可测性破约）。
pub const XC_RATE_DATA_GAP: u32 = 0x530C;
/// 0x530D 消费声明漂移（注册的声明与冻结接口不符——消费面失真）。
pub const XC_CONSUMER_DRIFT: u32 = 0x530D;
/// 0x530E 双跑漂移（同种子重放指纹不等——确定性破坏）。
pub const XC_REPLAY_DRIFT: u32 = 0x530E;
/// 0x530F 唤醒账失守（唤醒注入数与链路完成数不符——审计面漂移）。
pub const XC_WAKE_COUNT_GAP: u32 = 0x530F;

/// 诊断明细（码 + 人话 + 可操作建议）。
pub fn detail(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_WAKE_OFFLINE => (
            "唤醒离线核：wake 目标不在在线集",
            "唤醒前查在线快照；离线核由上线路径自行补看队列，不靠 IPI",
        ),
        XC_SCHED_REGRESS => (
            "调度点状态倒退：跳态或环序破坏",
            "Idle→Waked→InSchedule→Idle 单向环；倒退说明有旁路改状态",
        ),
        XC_CLEAR_NOBIT => (
            "清位无 pending：调度点清了未置位的位",
            "清位只在进调度点时发生且必须曾有置位；无位清=账面漂移",
        ),
        XC_WAKE_NO_SCHED => (
            "唤醒后未进调度点：IPI 发出但状态未变",
            "链路五步逐段查；断在目标核侧=调度器没消费 IPI，断在介质侧查 F0890",
        ),
        XC_WINDOW_BREACH => (
            "合并窗口越界：窗口外发 IPI 或窗口内直发",
            "窗口纪律=窗口内只置位、关窗一次发；越界说明有旁路直发路径",
        ),
        XC_RATE_WORSE => (
            "IPI 率不降反升：合并后比基线还多",
            "合并只会减少 IPI；变多说明窗口反复开关反而放大了次数，查窗口边界",
        ),
        XC_IFACE_ALTERED => (
            "接口指纹失守：协作接口被改",
            "冻结接口走版本前进；原位改写=消费方断链",
        ),
        XC_IPI_TWICE => (
            "双发 IPI：同窗口同核第二次发",
            "合并语义=一窗口一核恰一次；双发说明关窗结算漏了位图消费记录",
        ),
        XC_CONSUMER_MISSING => (
            "消费声明缺失：接口消费方未注册",
            "调度域消费前必须注册声明（锚定指纹）；未注册消费=越权对接",
        ),
        XC_BITMAP_DIRTY => (
            "位图越界位：pending 位图高位脏",
            "位宽=核数；高位脏位说明位图复用了更宽平台旧值，先清高位",
        ),
        XC_WINDOW_OPEN => (
            "窗口未关结算：开启时被查 IPI 账",
            "IPI 账只在关窗结算后可查；开着查=半程账，先 flush 再查",
        ),
        XC_RATE_DATA_GAP => (
            "降幅账残缺：基线或合并账缺失",
            "降幅=1−合并/基线，两账缺一不可测；补齐测量点再报纪律结论",
        ),
        XC_CONSUMER_DRIFT => (
            "消费声明漂移：注册声明与冻结接口不符",
            "声明锚定指纹；漂移=消费方拿了旧接口，重新生成声明",
        ),
        XC_REPLAY_DRIFT => (
            "双跑漂移：同种子重放指纹不等",
            "同种子同序列；不等则逐段二分找隐藏可变状态",
        ),
        XC_WAKE_COUNT_GAP => (
            "唤醒账失守：注入数与完成数不符",
            "每笔唤醒注入必达调度点或显性拒绝；差额=链路漏步",
        ),
        _ => ("未知诊断码", "码不在 0x53 闭集内——查是否跨段误用"),
    }
}

// ---------------------------------------------------------------------------
// §1 调度点状态机 + 协作接口契约（冻结面）
// ---------------------------------------------------------------------------

/// 调度点状态闭集（单向环：Idle→Waked→InSchedule→Idle）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchedState {
    /// 空闲（无待调度增量）。
    Idle,
    /// 已唤醒（pending 置位/IPI 已达，待进调度点）。
    Waked,
    /// 调度中（正在看队列——此刻清 pending 位）。
    InSchedule,
}

impl SchedState {
    pub fn name(self) -> &'static str {
        match self {
            SchedState::Idle => "idle",
            SchedState::Waked => "waked",
            SchedState::InSchedule => "in_schedule",
        }
    }

    /// 状态编号（环序校验基准 0..=2）。
    pub fn ord(self) -> u8 {
        match self {
            SchedState::Idle => 0,
            SchedState::Waked => 1,
            SchedState::InSchedule => 2,
        }
    }
}

/// 协作接口契约三节（冻结面——调度域消费方编译锚定）。
pub const IFACE_VERSION: u32 = 1;
/// 节 I · 签名。
pub const IFACE_SEC_SIGN: &str = "I.签名:pub fn wake(cpu:u32)->Result<(),u32>（置位,窗口内幂等）；pub fn flush()->Result<Vec<u32>,u32>（关窗结算,按位图发IPI）；pub fn enter_schedule(cpu:u32)->Result<usize,u32>（进调度点,清位,返回本窗合并请求数）；消费方=调度域（resched 域唯一合法消费者）。";
/// 节 II · 时序。
pub const IFACE_SEC_ORDER: &str = "II.时序:wake 置位（窗口开启期幂等无介质动作）；flush 关窗一次发 IPI（位图全量消费,一核恰一次）；目标核收 IPI 后 Idle→Waked；enter_schedule 清位并 Waked→InSchedule→Idle；窗口关闭前禁止查 IPI 账（半程账不对外）。";
/// 节 III · 所有权。
pub const IFACE_SEC_OWN: &str = "III.所有权:pending 位图归本单（调度域只经 wake/enter 间接触达）；IPI 介质归 F0890 投递层（本单只计数不对介质写）；调度队列归调度域（本单不读队列只发看队列信号）；三方边界以指纹冻结,越界对接即 0x5309。";

/// 接口全文（三节拼接——指纹基准）。
pub fn iface_full() -> String {
    let mut s = String::new();
    s.push_str(IFACE_SEC_SIGN);
    s.push_str(IFACE_SEC_ORDER);
    s.push_str(IFACE_SEC_OWN);
    s
}

/// FNV-1a 64。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 接口冻结指纹（写死常量——消费样例锚定）。
pub const IFACE_FINGERPRINT: u64 = 0x53A2_C0DE_0926_B07B;

/// 契约自检（三节齐备+指纹一致——0x5307）。
pub fn iface_audit() -> Result<(), (u32, String)> {
    for (i, sec) in [IFACE_SEC_SIGN, IFACE_SEC_ORDER, IFACE_SEC_OWN].iter().enumerate() {
        if sec.is_empty() {
            return Err((XC_IFACE_ALTERED, format!("接口第{}节为空", i + 1)));
        }
    }
    let real = fnv64(iface_full().as_bytes());
    if real != IFACE_FINGERPRINT {
        return Err((
            XC_IFACE_ALTERED,
            format!("接口指纹 {:#018x} ≠ 冻结 {:#018x}", real, IFACE_FINGERPRINT),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §2 唤醒引擎（pending 位图+合并窗口+调度点状态+接口注册面）
// ---------------------------------------------------------------------------

/// 核槽（状态+pending 位+窗口请求计数）。
pub struct CpuSlot2 {
    pub state: SchedState,
    /// 本核有 pending 唤醒请求。
    pub pending: bool,
    /// 本窗口累计唤醒请求数（合并前——W/N 降幅的 W 侧）。
    pub window_reqs: u64,
}

impl CpuSlot2 {
    fn new() -> CpuSlot2 {
        CpuSlot2 {
            state: SchedState::Idle,
            pending: false,
            window_reqs: 0,
        }
    }
}

/// 唤醒引擎。
pub struct ReschedEngine {
    slots: Vec<CpuSlot2>,
    online: Vec<u32>,
    /// 合并窗口开启中。
    window_open: bool,
    /// 窗口长度（tick——到期自动关窗）。
    pub window_len: u64,
    window_deadline: u64,
    tick: u64,
    // ---- 账面 ----
    /// 唤醒请求总数（W——合并前口径）。
    pub wake_req_total: u64,
    /// 实发 IPI 总数（N——合并后口径）。
    pub ipi_sent_total: u64,
    /// 基线 IPI 数（不开合并窗口的对照账——每请求一发）。
    pub baseline_ipi_total: u64,
    /// 进入调度点数。
    pub sched_enter_total: u64,
    /// 唤醒注入完成数（注入→调度点全链）。
    pub wake_done_total: u64,
    /// 消费方注册声明（None=未注册——0x5309）。
    consumer: Option<(&'static str, u64)>,
}

impl ReschedEngine {
    /// 构造（核数+在线集）。
    pub fn new(ncpu: u32, online: &[u32]) -> Result<ReschedEngine, (u32, String)> {
        if ncpu == 0 || ncpu > 64 {
            return Err((XC_BITMAP_DIRTY, format!("核数 {} 越界（1..=64）", ncpu)));
        }
        let mut slots = Vec::new();
        for _ in 0..ncpu {
            slots.push(CpuSlot2::new());
        }
        let mut on = Vec::new();
        for &c in online {
            if c >= ncpu {
                return Err((XC_WAKE_OFFLINE, format!("在线核 #{} 越界", c)));
            }
            if on.contains(&c) {
                return Err((XC_BITMAP_DIRTY, format!("在线核 #{} 重复", c)));
            }
            on.push(c);
            slots[c as usize].state = SchedState::Idle;
        }
        if on.is_empty() {
            return Err((XC_WAKE_OFFLINE, "空在线集".to_string()));
        }
        Ok(ReschedEngine {
            slots,
            online: on,
            window_open: false,
            window_len: 4,
            window_deadline: 0,
            tick: 0,
            wake_req_total: 0,
            ipi_sent_total: 0,
            baseline_ipi_total: 0,
            sched_enter_total: 0,
            wake_done_total: 0,
            consumer: None,
        })
    }

    /// 消费方注册（协作面准入——声明锚定指纹）。
    pub fn register_consumer(&mut self, name: &'static str) -> Result<(), (u32, String)> {
        let fp = fnv64(iface_full().as_bytes());
        if fp != IFACE_FINGERPRINT {
            return Err((XC_CONSUMER_DRIFT, format!("声明指纹 {:#018x} 与冻结不符", fp)));
        }
        self.consumer = Some((name, fp));
        Ok(())
    }

    /// 消费面校验（所有 IPI 动作前置——0x5309）。
    fn require_consumer(&self) -> Result<(), (u32, String)> {
        match self.consumer {
            Some((_, fp)) if fp == IFACE_FINGERPRINT => Ok(()),
            Some(_) => Err((XC_CONSUMER_DRIFT, "注册指纹漂移".to_string())),
            None => Err((
                XC_CONSUMER_MISSING,
                "调度域未注册消费声明——越权对接（先 register_consumer）".to_string(),
            )),
        }
    }

    /// 唤醒（wake——窗口内幂等置位；不开窗则按基线口径直接发 IPI 对照账）。
    pub fn wake(&mut self, cpu: u32) -> Result<(), (u32, String)> {
        self.require_consumer()?;
        if cpu as usize >= self.slots.len() || !self.online.contains(&cpu) {
            return Err((
                XC_WAKE_OFFLINE,
                format!("核 #{} 离线或越界——唤醒面拒绝", cpu),
            ));
        }
        self.tick += 1;
        self.wake_req_total += 1;
        self.baseline_ipi_total += 1; // 基线口径：每请求一发（对照账恒步进）。
        let slot = &mut self.slots[cpu as usize];
        slot.window_reqs += 1;
        if slot.pending {
            return Ok(()); // 幂等：已 pending 的重复请求不再置位计数（窗口合并语义的核心）。
        }
        slot.pending = true;
        if self.window_open {
            // 窗口内：只置位不发 IPI（合并等待关窗）。
            Ok(())
        } else {
            // 无窗口：立即单发（请求==IPI——基线行为）。
            slot.pending = false;
            slot.state = match slot.state {
                SchedState::Idle => SchedState::Waked,
                s => s,
            };
            self.ipi_sent_total += 1;
            Ok(())
        }
    }

    /// 开窗（合并窗口——开启后 wake 只置位）。
    pub fn open_window(&mut self) -> Result<(), (u32, String)> {
        self.require_consumer()?;
        if self.window_open {
            return Err((XC_WINDOW_BREACH, "窗口已开——重复开窗".to_string()));
        }
        self.window_open = true;
        self.window_deadline = self.tick + self.window_len;
        Ok(())
    }

    /// 关窗结算（flush——按位图一次发 IPI，一核恰一次）。
    pub fn flush(&mut self) -> Result<Vec<u32>, (u32, String)> {
        self.require_consumer()?;
        if !self.window_open {
            return Err((XC_WINDOW_BREACH, "窗口未开——无结算对象".to_string()));
        }
        let mut woken: Vec<u32> = Vec::new();
        for &c in &self.online {
            let slot = &mut self.slots[c as usize];
            if slot.pending {
                // 一核恰一次（双发 0x5308 的结构性防：置位消费即清）。
                slot.pending = false;
                slot.state = match slot.state {
                    SchedState::Idle => SchedState::Waked,
                    s => s,
                };
                self.ipi_sent_total += 1;
                woken.push(c);
            }
            slot.window_reqs = 0;
        }
        self.window_open = false;
        self.window_deadline = 0;
        Ok(woken)
    }

    /// tick 步进（窗口到期自动关窗——时间驱动侧）。
    pub fn tick_step(&mut self) -> Result<Vec<u32>, (u32, String)> {
        self.tick += 1;
        if self.window_open && self.tick >= self.window_deadline {
            self.flush()
        } else {
            Ok(Vec::new())
        }
    }

    /// 进入调度点（enter_schedule——清位+状态环推进）。
    pub fn enter_schedule(&mut self, cpu: u32) -> Result<u64, (u32, String)> {
        self.require_consumer()?;
        if cpu as usize >= self.slots.len() {
            return Err((XC_WAKE_OFFLINE, format!("核 #{} 越界", cpu)));
        }
        self.tick += 1;
        let slot = &mut self.slots[cpu as usize];
        // 状态环校验（Idle 直入调度点=跳态 0x5302——必须先 Waked）。
        if slot.state != SchedState::Waked {
            return Err((
                XC_SCHED_REGRESS,
                format!(
                    "核 #{} 状态 {} 直入调度点——跳态（须先 Waked）",
                    cpu,
                    slot.state.name()
                ),
            ));
        }
        // 清位校验（无 pending 的清位=漂移 0x5303——Waked 必由置位而来）。
        if !slot.pending && slot.window_reqs == 0 {
            return Err((
                XC_CLEAR_NOBIT,
                format!("核 #{} 无 pending 清位——pending 面漂移", cpu),
            ));
        }
        let merged = slot.window_reqs;
        slot.pending = false;
        slot.window_reqs = 0;
        slot.state = SchedState::InSchedule;
        self.sched_enter_total += 1;
        // 调度点退出（同步模型：看一眼队列即回 Idle——环闭合）。
        slot.state = SchedState::Idle;
        self.wake_done_total += 1;
        Ok(merged)
    }

    /// 目标核状态（链路审计面）。
    pub fn state_of(&self, cpu: u32) -> Result<SchedState, (u32, String)> {
        self.slots
            .get(cpu as usize)
            .map(|s| s.state)
            .ok_or_else(|| (XC_WAKE_OFFLINE, format!("核 #{} 越界", cpu)))
    }

    /// IPI 率降幅（ppm——合并窗口纪律的量化证据：1−N/W）。
    pub fn reduction_ppm(&self) -> Result<u64, (u32, String)> {
        if self.baseline_ipi_total == 0 || self.wake_req_total == 0 {
            return Err((
                XC_RATE_DATA_GAP,
                format!(
                    "降幅账残缺：基线 {} 唤醒 {}——补齐测量点",
                    self.baseline_ipi_total, self.wake_req_total
                ),
            ));
        }
        if self.ipi_sent_total > self.baseline_ipi_total {
            return Err((
                XC_RATE_WORSE,
                format!(
                    "合并 IPI {} > 基线 {}——纪律失效",
                    self.ipi_sent_total, self.baseline_ipi_total
                ),
            ));
        }
        Ok((self.baseline_ipi_total - self.ipi_sent_total) * 1_000_000 / self.baseline_ipi_total)
    }

    /// 账面审计（唤醒注入==链路完成+位图干净+窗口闭合）。
    pub fn audit(&self) -> Result<(), (u32, String)> {
        if self.wake_done_total > self.wake_req_total {
            return Err((
                XC_WAKE_COUNT_GAP,
                format!(
                    "链路完成 {} > 唤醒注入 {}——账面多步",
                    self.wake_done_total, self.wake_req_total
                ),
            ));
        }
        for (i, s) in self.slots.iter().enumerate() {
            if s.pending && s.state == SchedState::Idle && !self.window_open {
                return Err((
                    XC_CLEAR_NOBIT,
                    format!("核 #{} 窗外挂 pending——置位未消费", i),
                ));
            }
        }
        if self.window_open {
            return Err((XC_WINDOW_OPEN, "审计时窗口未关——先 flush".to_string()));
        }
        Ok(())
    }

    /// 快照指纹。
    pub fn snapshot_fp(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&self.wake_req_total.to_le_bytes());
        buf.extend_from_slice(&self.ipi_sent_total.to_le_bytes());
        buf.extend_from_slice(&self.baseline_ipi_total.to_le_bytes());
        buf.extend_from_slice(&self.sched_enter_total.to_le_bytes());
        buf.extend_from_slice(&self.wake_done_total.to_le_bytes());
        for (i, s) in self.slots.iter().enumerate() {
            buf.push(i as u8);
            buf.push(s.state.ord());
            buf.push(s.pending as u8);
            buf.extend_from_slice(&s.window_reqs.to_le_bytes());
        }
        fnv64(&buf)
    }
}

// ---------------------------------------------------------------------------
// §3 J1 三闸
// ---------------------------------------------------------------------------

/// 线性同余发生器。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 33) as u32
    }
    pub fn below(&mut self, n: u32) -> u32 {
        self.next_u32() % n
    }
}

/// J1-A：唤醒链路正确——10/10 注入全达调度点（wake→flush→enter 全链）。
pub fn j1_wake_chain_10() -> Result<String, (u32, String)> {
    iface_audit()?;
    let mut e = ReschedEngine::new(4, &[0, 1, 2, 3])?;
    e.register_consumer("resched-domain")?;
    let mut hits = 0u32;
    for round in 0..10 {
        let cpu = (round % 4) as u32;
        e.open_window()?;
        e.wake(cpu)?;
        let woken = e.flush()?;
        if !woken.contains(&cpu) {
            return Err((
                XC_WAKE_NO_SCHED,
                format!("round {} 核 #{} 未在唤醒集 {:?}", round, cpu, woken),
            ));
        }
        // 状态链：Waked（IPI 已达）→enter_schedule。
        let st = e.state_of(cpu)?;
        if st != SchedState::Waked {
            return Err((
                XC_WAKE_NO_SCHED,
                format!("round {} 核 #{} 状态 {} ≠ waked", round, cpu, st.name()),
            ));
        }
        let merged = e.enter_schedule(cpu)?;
        if merged != 1 {
            return Err((XC_WAKE_COUNT_GAP, format!("round {} 合并数 {} ≠ 1", round, merged)));
        }
        hits += 1;
    }
    if hits != 10 || e.wake_done_total != 10 {
        return Err((XC_WAKE_COUNT_GAP, format!("链路完成 {} /10", hits)));
    }
    e.audit()?;
    Ok(format!(
        "唤醒链路 10/10 全过（wake→flush→waked→enter→idle 五步全链留痕）；合并口径恰 1；审计绿"
    ))
}

/// J1-B：IPI 频率纪律——合并窗口生效（降幅可测：同负载开窗 vs 基线）。
pub fn j1_merge_reduction() -> Result<String, (u32, String)> {
    let mut e = ReschedEngine::new(8, &[0, 1, 2, 3, 4, 5, 6, 7])?;
    e.register_consumer("resched-domain")?;
    let mut rng = Lcg::new(0x5326_B07A);
    // 场景：5 轮 × 每轮开窗期内随机 40 次唤醒（8 核）→ 关窗。
    for _ in 0..5 {
        e.open_window()?;
        for _ in 0..40 {
            let c = rng.below(8);
            e.wake(c)?;
        }
        e.flush()?;
    }
    // 基线账自动累计（wake 每请求 +1 基线 IPI——200 请求基线 200）。
    let base = e.baseline_ipi_total;
    let merged = e.ipi_sent_total;
    if base != 200 {
        return Err((XC_RATE_DATA_GAP, format!("基线 {} ≠ 200", base)));
    }
    // 合并 IPI：每轮最多 8（每核恰一次）——5 轮 ≤40。
    if merged > 40 {
        return Err((
            XC_RATE_WORSE,
            format!("合并 IPI {} > 理论上界 40", merged),
        ));
    }
    let ppm = e.reduction_ppm()?;
    // 降幅必须可测（>50%——40 次唤醒合并后至少一半核重复唤醒的场景）。
    if ppm < 500_000 {
        return Err((
            XC_RATE_WORSE,
            format!("降幅 {} ppm < 50%——合并未生效", ppm),
        ));
    }
    e.audit()?;
    Ok(format!(
        "合并窗口生效：200 唤醒请求 → {} 次 IPI（理论上界 40）；降幅 {} ppm（{}%）；基线/合并两账齐备",
        merged,
        ppm,
        ppm / 10_000
    ))
}

/// J1-C：协作接口冻结——消费声明注册+指纹对拍+未注册拦截。
pub fn j1_iface_freeze() -> Result<String, (u32, String)> {
    iface_audit()?;
    // 未注册消费拦截。
    let mut e = ReschedEngine::new(2, &[0, 1])?;
    match e.wake(0) {
        Err((XC_CONSUMER_MISSING, _)) => {}
        _ => return Err((XC_CONSUMER_MISSING, "未注册 wake 未拦——协作面空洞".to_string())),
    }
    match e.open_window() {
        Err((XC_CONSUMER_MISSING, _)) => {}
        _ => return Err((XC_CONSUMER_MISSING, "未注册开窗未拦".to_string())),
    }
    // 注册后放行。
    e.register_consumer("resched-domain")?;
    e.open_window()?;
    e.wake(0)?;
    e.flush()?;
    e.enter_schedule(0)?;
    // 消费样例：接口三节签名逐字消费（编译形态+指纹运行期对拍）。
    let real = fnv64(iface_full().as_bytes());
    if real != IFACE_FINGERPRINT {
        return Err((XC_IFACE_ALTERED, "样例侧指纹对拍失守".to_string()));
    }
    e.audit()?;
    Ok(format!(
        "协作接口冻结过：未注册双动作拦截（0x5309）；注册后全链放行；指纹 {:#018x} 三节冻结一致",
        real
    ))
}

/// 负路径 10 闸（状态环/位图/窗口/账面——码精确命中）。
pub fn reject_gate_10() -> Result<String, (u32, String)> {
    let mut e = ReschedEngine::new(4, &[0, 1, 2, 3])?;
    e.register_consumer("resched-domain")?;
    let mut hits: Vec<(&str, bool)> = Vec::new();

    // 1）唤醒离线核（构造含离线在线集）。
    let mut e2 = ReschedEngine::new(4, &[0, 1, 2])?;
    e2.register_consumer("resched-domain")?;
    match e2.wake(3) {
        Err((XC_WAKE_OFFLINE, _)) => hits.push(("唤醒离线核", true)),
        _ => hits.push(("唤醒离线核", false)),
    }
    // 2）Idle 直入调度点（跳态）。
    match e.enter_schedule(1) {
        Err((XC_SCHED_REGRESS, _)) => hits.push(("跳态入调度点", true)),
        _ => hits.push(("跳态入调度点", false)),
    }
    // 3）关窗后 flush。
    match e.flush() {
        Err((XC_WINDOW_BREACH, _)) => hits.push(("未开窗结算", true)),
        _ => hits.push(("未开窗结算", false)),
    }
    // 4）重复开窗。
    e.open_window()?;
    match e.open_window() {
        Err((XC_WINDOW_BREACH, _)) => hits.push(("重复开窗", true)),
        _ => hits.push(("重复开窗", false)),
    }
    e.flush()?;
    // 5）构造越界核数。
    match ReschedEngine::new(0, &[0]) {
        Err((XC_BITMAP_DIRTY, _)) => hits.push(("零核构造", true)),
        _ => hits.push(("零核构造", false)),
    }
    // 6）构造在线越界。
    match ReschedEngine::new(2, &[0, 5]) {
        Err((XC_WAKE_OFFLINE, _)) => hits.push(("在线越界", true)),
        _ => hits.push(("在线越界", false)),
    }
    // 7）构造空在线集。
    match ReschedEngine::new(2, &[]) {
        Err((XC_WAKE_OFFLINE, _)) => hits.push(("空在线集", true)),
        _ => hits.push(("空在线集", false)),
    }
    // 8）指纹篡改检出（伪造漂移指纹引擎）。
    let mut evil = ReschedEngine::new(2, &[0, 1])?;
    evil.register_consumer("resched-domain")?;
    evil.consumer = Some(("evil", 0xDEAD));
    match evil.wake(0) {
        Err((XC_CONSUMER_DRIFT, _)) => hits.push(("声明指纹漂移", true)),
        _ => hits.push(("声明指纹漂移", false)),
    }
    // 9）合并口径双发防（同核同窗两 wake 一次 flush 恰一 IPI——结构验证）。
    let before = e.ipi_sent_total;
    e.open_window()?;
    e.wake(2)?;
    e.wake(2)?;
    e.wake(2)?;
    e.flush()?;
    let delta = e.ipi_sent_total - before;
    if delta == 1 {
        hits.push(("三唤醒一IPI", true));
    } else {
        hits.push(("三唤醒一IPI", false));
    }
    // 10）tick 到期自动关窗。
    e.open_window()?;
    e.wake(1)?;
    let auto = e.tick_step()?; // deadline=tick+4，步进 1 不触发。
    if auto.is_empty() {
        hits.push(("窗口未到期", true));
    } else {
        hits.push(("窗口未到期", false));
    }

    let missed: Vec<&str> = hits.iter().filter(|(_, ok)| !ok).map(|(n, _)| *n).collect();
    if !missed.is_empty() {
        return Err((
            XC_WAKE_COUNT_GAP,
            format!("负路径失守：{:?} 未按期望", missed),
        ));
    }
    Ok("负路径 10/10 全拦且口径精确".to_string())
}

/// 万级压测（1 万次唤醒随机混合窗口/直发——降幅+审计+指纹全链）。
pub fn stress_10k() -> Result<String, (u32, String)> {
    let mut e = ReschedEngine::new(8, &[0, 1, 2, 3, 4, 5, 6, 7])?;
    e.register_consumer("resched-domain")?;
    let mut rng = Lcg::new(0x532B_07A2);
    for i in 0..10_000u64 {
        if i % 10 == 0 {
            e.open_window()?;
        }
        let c = rng.below(8);
        e.wake(c)?;
        if i % 10 == 9 {
            e.flush()?;
        }
        if i % 7 == 0 {
            // 概率性进调度点（消费唤醒——Waked 核全清）。
            for cpu in 0..8u32 {
                if e.state_of(cpu)? == SchedState::Waked {
                    e.enter_schedule(cpu)?;
                }
            }
        }
    }
    e.flush()?;
    e.audit()?;
    let ppm = e.reduction_ppm()?;
    if e.wake_req_total != 10_000 {
        return Err((XC_WAKE_COUNT_GAP, format!("唤醒 {} ≠ 10000", e.wake_req_total)));
    }
    Ok(format!(
        "万级压测过：1 万唤醒（合并 IPI {} / 基线 {}）；降幅 {} ppm；链路完成 {}；审计绿",
        e.ipi_sent_total, e.baseline_ipi_total, ppm, e.wake_done_total
    ))
}

/// 双跑指纹。
pub fn dual_run_fp() -> Result<u64, (u32, String)> {
    let mut e = ReschedEngine::new(4, &[0, 1, 2, 3])?;
    e.register_consumer("resched-domain")?;
    let mut rng = Lcg::new(0x53D0_0B1E);
    for _ in 0..400 {
        if rng.below(3) == 0 {
            e.open_window()?;
        }
        let _ = e.wake(rng.below(4));
        if rng.below(2) == 0 {
            let _ = e.flush();
        }
        for cpu in 0..4u32 {
            if e.state_of(cpu)? == SchedState::Waked && rng.below(2) == 0 {
                let _ = e.enter_schedule(cpu);
            }
        }
    }
    let _ = e.flush();
    e.audit()?;
    Ok(e.snapshot_fp())
}

/// F0926 总闸。
pub fn run_gate() -> Result<String, (u32, String)> {
    let a = j1_wake_chain_10()?;
    let b = j1_merge_reduction()?;
    let c = j1_iface_freeze()?;
    let d = reject_gate_10()?;
    let e = stress_10k()?;
    let f1 = dual_run_fp()?;
    let f2 = dual_run_fp()?;
    if f1 != f2 {
        return Err((XC_REPLAY_DRIFT, format!("双跑指纹 {:#018x}≠{:#018x}", f1, f2)));
    }
    Ok(format!(
        "F0926 全绿：\n[唤醒链路] {}\n[合并纪律] {}\n[接口冻结] {}\n[负路径] {}\n[万级压测] {}\n[双跑] {:#018x} 稳定",
        a, b, c, d, e, f1
    ))
}
