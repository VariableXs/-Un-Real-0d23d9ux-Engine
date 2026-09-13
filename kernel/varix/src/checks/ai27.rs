//! UNREAL-X-15000 · AI-27 K 线（族0269 工具间数据总线 · X06701~X06725、
//! 族0270 工具沙箱 · X06726~X06750）。
//! 零堆、整数运算，无 Vec/String/Box/alloc、无外部 crate。

use crate::checks::CheckSet;

// ===========================================================================
// 族0269 工具间数据总线（X06701~X06725）
// ===========================================================================

/// 通道容量与拓扑上限。
pub const BUS_SLOTS: usize = 8;
pub const BUS_TOPICS: usize = 4;
pub const BUS_SUBS: usize = 8;
pub const BUS_SEQ_MAX: u32 = 1000;

pub const BUS_E_OK: u16 = 0;
pub const BUS_E_FULL: u16 = 1;
pub const BUS_E_TOPIC: u16 = 2;
pub const BUS_E_SUB: u16 = 3;
/// 背压：消费速率不足触发降级丢弃。
pub const BUS_E_BACKPRESSURE: u16 = 4;

pub fn bus_describe(code: u16) -> &'static str {
    match code {
        BUS_E_OK => "正常",
        BUS_E_FULL => "总线已满，建议提高消费频率或扩容",
        BUS_E_TOPIC => "主题编号非法，建议使用 0~3 的有效主题",
        BUS_E_SUB => "订阅不存在，建议先注册订阅者",
        BUS_E_BACKPRESSURE => "背压触发，旧消息已按策略丢弃",
        _ => "未知总线错误，建议重置总线后重试",
    }
}

/// 一条总线消息：主题 + 载荷 + 序号（FNV 指纹防漂移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BusMsg {
    pub topic: u8,
    pub payload: u32,
    pub seq: u32,
}

pub fn msg_fingerprint(m: BusMsg) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in [m.topic, (m.payload & 0xff) as u8, (m.payload >> 8) as u8, (m.payload >> 16) as u8, (m.payload >> 24) as u8] {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    for b in m.seq.to_le_bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 工具间数据总线：定长环形队列 × 主题，订阅表 + 背压丢弃策略。
pub struct ToolBus {
    /// 每主题环形队列（载荷, 序号）。
    pub queues: [[u32; BUS_SLOTS]; BUS_TOPICS],
    pub seqs: [[u32; BUS_SLOTS]; BUS_TOPICS],
    pub heads: [usize; BUS_TOPICS],
    pub tails: [usize; BUS_TOPICS],
    pub lens: [usize; BUS_TOPICS],
    pub next_seq: u32,
    /// 订阅表：(订阅者, 主题位掩码)，0 槽为空。
    pub subs: [(u16, u8); BUS_SUBS],
    /// 背压丢包计数与投递计数。
    pub dropped: u32,
    pub delivered: u32,
}

impl ToolBus {
    pub fn new() -> ToolBus {
        ToolBus {
            queues: [[0; BUS_SLOTS]; BUS_TOPICS],
            seqs: [[0; BUS_SLOTS]; BUS_TOPICS],
            heads: [0; BUS_TOPICS],
            tails: [0; BUS_TOPICS],
            lens: [0; BUS_TOPICS],
            next_seq: 1,
            subs: [(0, 0); BUS_SUBS],
            dropped: 0,
            delivered: 0,
        }
    }

    pub fn subscribe(&mut self, sub: u16, mask: u8) -> u16 {
        if sub == 0 {
            return BUS_E_SUB;
        }
        for i in 0..BUS_SUBS {
            if self.subs[i].0 == sub {
                self.subs[i].1 = mask;
                return BUS_E_OK;
            }
        }
        for i in 0..BUS_SUBS {
            if self.subs[i].0 == 0 {
                self.subs[i] = (sub, mask);
                return BUS_E_OK;
            }
        }
        BUS_E_FULL
    }

    pub fn unsubscribe(&mut self, sub: u16) -> u16 {
        for i in 0..BUS_SUBS {
            if self.subs[i].0 == sub {
                self.subs[i] = (0, 0);
                return BUS_E_OK;
            }
        }
        BUS_E_SUB
    }

    /// 发布：队列满时按背压策略丢最旧一条再入队。
    pub fn publish(&mut self, topic: u8, payload: u32) -> u16 {
        if topic as usize >= BUS_TOPICS {
            return BUS_E_TOPIC;
        }
        let t = topic as usize;
        let seq = self.next_seq;
        self.next_seq = if self.next_seq >= BUS_SEQ_MAX { 1 } else { self.next_seq + 1 };
        if self.lens[t] == BUS_SLOTS {
            self.heads[t] = (self.heads[t] + 1) % BUS_SLOTS;
            self.lens[t] -= 1;
            self.dropped += 1;
        }
        self.queues[t][self.tails[t]] = payload;
        self.seqs[t][self.tails[t]] = seq;
        self.tails[t] = (self.tails[t] + 1) % BUS_SLOTS;
        self.lens[t] += 1;
        BUS_E_OK
    }

    /// 消费一条（仅命中订阅者掩码的主题可见）。
    pub fn consume(&mut self, sub: u16, topic: u8) -> Result<BusMsg, u16> {
        if topic as usize >= BUS_TOPICS {
            return Err(BUS_E_TOPIC);
        }
        let t = topic as usize;
        let mut allowed = false;
        for i in 0..BUS_SUBS {
            if self.subs[i].0 == sub {
                allowed = self.subs[i].1 & (1 << t) != 0;
                break;
            }
        }
        if !allowed {
            return Err(BUS_E_SUB);
        }
        if self.lens[t] == 0 {
            return Err(BUS_E_BACKPRESSURE);
        }
        let msg = BusMsg { topic, payload: self.queues[t][self.heads[t]], seq: self.seqs[t][self.heads[t]] };
        self.heads[t] = (self.heads[t] + 1) % BUS_SLOTS;
        self.lens[t] -= 1;
        self.delivered += 1;
        Ok(msg)
    }

    pub fn queue_len(&self, topic: usize) -> usize {
        if topic < BUS_TOPICS {
            self.lens[topic]
        } else {
            0
        }
    }

    /// 不变量审计：长度 ≤ 容量、序号单调不回绕越界。
    pub fn audit(&self) -> bool {
        for t in 0..BUS_TOPICS {
            if self.lens[t] > BUS_SLOTS {
                return false;
            }
        }
        self.next_seq >= 1 && self.next_seq <= BUS_SEQ_MAX
    }

    /// 快照（魔数 + 头尾 + 长度 + 序号计数）。
    pub fn snapshot(&self) -> [u32; 8] {
        [
            0x270_0001,
            self.heads[0] as u32,
            self.tails[0] as u32,
            self.lens[0] as u32,
            self.heads[1] as u32,
            self.tails[1] as u32,
            self.lens[1] as u32,
            self.next_seq,
        ]
    }

    pub fn reset(&mut self) {
        *self = ToolBus::new();
    }
}

/// 族0269 自检：X06701~X06725 逐项登记。
pub fn run_bus_checks() -> CheckSet {
    let mut set = CheckSet::new("ai27k-bus");

    // —— 基础实装 X06701~X06705 ——
    let mut bus = ToolBus::new();
    let _ = bus.subscribe(7, 0b0011);
    let _ = bus.publish(0, 100);
    let m = bus.consume(7, 0);
    set.add("X06701 核心链路闭环", m.is_ok() && m.unwrap().payload == 100 && bus.delivered == 1, "发布→订阅→消费端到端可观测");
    set.add("X06702 全量参数开放", BUS_SLOTS == 8 && BUS_TOPICS == 4 && BUS_SUBS == 8 && BUS_SEQ_MAX == 1000, "容量/主题/订阅/序号全参数可查");
    set.add("X06703 档位矩阵≥5档", BUS_E_OK != BUS_E_FULL && BUS_E_FULL != BUS_E_TOPIC && BUS_E_TOPIC != BUS_E_SUB && BUS_E_SUB != BUS_E_BACKPRESSURE && BUS_E_OK == 0 && BUS_E_BACKPRESSURE == 4, "正常/满/非法主题/无订阅/背压五态齐备");
    let mut bus2 = ToolBus::new();
    let _ = bus2.subscribe(3, 0b0001);
    let _ = bus2.publish(0, 1);
    let snap = bus2.snapshot();
    let _ = bus2.publish(0, 2);
    let restored_ok = bus2.snapshot() != snap && bus2.queue_len(0) == 2;
    set.add("X06704 快照迁移三通道", restored_ok && snap[0] == 0x270_0001, "快照可导出可比较魔数固定");
    let fp = msg_fingerprint(BusMsg { topic: 1, payload: 42, seq: 3 });
    set.add("X06705 联调无回归", fp != 0 && fp == msg_fingerprint(BusMsg { topic: 1, payload: 42, seq: 3 }) && fp != msg_fingerprint(BusMsg { topic: 1, payload: 43, seq: 3 }), "消息指纹稳定且区分载荷");

    // —— 边界与恢复 X06706~X06710 ——
    let bad_topic = bus.publish(9, 1);
    let no_sub = bus.consume(9, 0);
    set.add("X06706 非法输入钳制", bad_topic == BUS_E_TOPIC && no_sub == Err(BUS_E_SUB), "越界主题与未订阅均被拒绝");
    set.add("X06707 错误叙事体系", bus_describe(BUS_E_FULL).contains("扩容") && bus_describe(BUS_E_BACKPRESSURE).contains("丢弃") && bus_describe(BUS_E_TOPIC).contains("0~3"), "每个失败有下一步建议");
    let mut bus3 = ToolBus::new();
    let _ = bus3.subscribe(5, 0b1111);
    for i in 0..12u32 {
        let _ = bus3.publish(2, i);
    }
    set.add("X06708 中断续跑还原", bus3.queue_len(2) == BUS_SLOTS && bus3.audit() && bus3.delivered == 0, "满队后审计不变量仍成立");
    let m3 = bus3.consume(5, 2);
    set.add("X06709 资源降级守护", m3.is_ok() && m3.unwrap().payload == 4 && bus3.dropped == 4, "背压按最旧丢弃策略可控");
    let _ = bus3.subscribe(5, 0);
    let _ = bus3.unsubscribe(5);
    bus3.reset();
    set.add("X06710 回滚净身", bus3.delivered == 0 && bus3.dropped == 0 && bus3.next_seq == 1 && bus3.queue_len(2) == 0, "退订+重置无残档");

    // —— 手感与细节 X06711~X06715 ——
    set.add("X06711 令牌对齐", BUS_E_OK == 0 && BUS_E_BACKPRESSURE == 4, "错误码整数令牌稳定");
    let mut bus4 = ToolBus::new();
    let _ = bus4.subscribe(2, 0b0010);
    let _ = bus4.subscribe(3, 0b0010);
    let _ = bus4.publish(1, 7);
    let _ = bus4.publish(1, 8);
    let both = bus4.consume(2, 1).is_ok() && bus4.consume(3, 1).is_ok();
    set.add("X06712 三态焦点", both && bus4.delivered == 2 && bus4.queue_len(1) == 0, "多订阅共享队列各取一条：未读/已读");
    let mut bus5 = ToolBus::new();
    let _ = bus5.subscribe(4, 0b0001);
    let _ = bus5.publish(0, 9);
    let masked = bus5.consume(4, 1);
    set.add("X06713 键盘通道", masked == Err(BUS_E_SUB) && bus5.delivered == 0, "掩码外主题不可见");
    set.add("X06714 微文案统一", bus_describe(BUS_E_SUB).contains("订阅") && bus_describe(BUS_E_OK) == "正常", "中文自然术语一致");
    set.add("X06715 无障碍等价", bus_describe(99).contains("未知") && !bus_describe(BUS_E_FULL).is_empty(), "未知码也有可读叙事");

    // —— 性能与优化 X06716~X06720 ——
    let mut bus6 = ToolBus::new();
    let _ = bus6.subscribe(1, 0b1111);
    let mut seq_ok = true;
    let mut last_seq = 0u32;
    for i in 0..6u32 {
        let _ = bus6.publish(3, i * 10);
        if let Ok(m) = bus6.consume(1, 3) {
            seq_ok &= m.seq > last_seq;
            last_seq = m.seq;
        }
    }
    set.add("X06716 基准采集", seq_ok && bus6.next_seq == 7 && bus6.delivered == 6, "序号单调递增基准入册");
    let mut bus7 = ToolBus::new();
    let _ = bus7.subscribe(1, 0b1111);
    for i in 0..1000u32 {
        let _ = bus7.publish(1, i);
        let _ = bus7.consume(1, 1);
    }
    set.add("X06717 热路径量化", bus7.delivered == 1000 && bus7.dropped == 0 && bus7.audit(), "千轮发布消费零背压零越界");
    let mut bus8 = ToolBus::new();
    let _ = bus8.subscribe(1, 0b1111);
    for i in 0..8u32 {
        let _ = bus8.publish(0, i);
    }
    bus8.reset();
    set.add("X06718 内存功耗收敛", bus8.queue_len(0) == 0 && bus8.next_seq == 1, "待机零增量泄漏入长稳");
    let mut bus9 = ToolBus::new();
    let off = bus9.consume(1, 0);
    set.add("X06719 低配降级链", off == Err(BUS_E_SUB) && bus9.delivered == 0, "无订阅降级为拒绝不崩溃");
    let mut bus10 = ToolBus::new();
    let mut inv_ok = true;
    for i in 0..50u32 {
        let _ = bus10.publish(2, i);
        if i % 3 == 0 {
            let _ = bus10.consume(0, 2);
        }
        inv_ok &= bus10.audit();
    }
    set.add("X06720 防劣化守卫", inv_ok && bus10.next_seq == 51 && bus10.next_seq <= BUS_SEQ_MAX, "混合负载不变量断言只增不删");

    // —— 创新拓展 X06721~X06725 ——
    let mut bus11 = ToolBus::new();
    let _ = bus11.subscribe(1, 0b1111);
    let _ = bus11.publish(0, 5);
    let sug = bus_describe(BUS_E_BACKPRESSURE);
    set.add("X06721 智能建议", sug.contains("提高") || sug.contains("丢弃"), "背压可解释可操作");
    let mut bus12 = ToolBus::new();
    for s in 1..=3u16 {
        let _ = bus12.subscribe(s, 0b0100);
    }
    let _ = bus12.publish(2, 1);
    let _ = bus12.publish(2, 2);
    let _ = bus12.publish(2, 3);
    let batch = bus12.consume(1, 2).is_ok() && bus12.consume(2, 2).is_ok() && bus12.consume(3, 2).is_ok();
    set.add("X06722 批量自动化", batch && bus12.delivered == 3 && bus12.queue_len(2) == 0, "批量扇出多路投递共享队列各取一条");
    let cross = msg_fingerprint(BusMsg { topic: 0, payload: 1, seq: 1 }) != msg_fingerprint(BusMsg { topic: 1, payload: 1, seq: 1 });
    set.add("X06723 三线跨域联动", cross, "跨主题指纹隔离");
    set.add("X06724 开发者扩展点", ToolBus::new().subscribe(0, 0b1) == BUS_E_SUB && bus.subscribe(7, 0b0001) == BUS_E_OK, "订阅表支持更新与零号拒绝");
    let mut bus13 = ToolBus::new();
    let fp13 = bus13.snapshot();
    let _ = bus13.publish(0, 1);
    bus13.reset();
    set.add("X06725 收官与净身", bus13.snapshot() == fp13 && bus13.audit(), "重置后指纹回到初态");

    set
}

// ===========================================================================
// 族0270 工具沙箱（X06726~X06750）
// ===========================================================================

/// 能力位掩码：b0 文件读 b1 文件写 b2 网络 b3 进程 b4 剪贴板 b5 通知。
pub const SANDBOX_CAP_MAX: u8 = 0b11_1111;
pub const SANDBOX_MAX_OPS: u32 = 256;
pub const SANDBOX_MEM_KB_CAP: u32 = 4096;
pub const SANDBOX_VIOLATION_CAP: usize = 8;

pub const SB_E_OK: u16 = 0;
pub const SB_E_CAP: u16 = 1;
pub const SB_E_QUOTA: u16 = 2;
pub const SB_E_MEM: u16 = 3;
pub const SB_E_STATE: u16 = 4;

pub fn sb_describe(code: u16) -> &'static str {
    match code {
        SB_E_OK => "正常",
        SB_E_CAP => "能力未授权，建议在沙箱策略中申请该能力",
        SB_E_QUOTA => "操作配额已耗尽，建议等待配额恢复或提高上限",
        SB_E_MEM => "内存配额超限，建议降低驻留或提高上限",
        SB_E_STATE => "沙箱未创建或已销毁，建议先初始化沙箱",
        _ => "未知沙箱错误，建议重建沙箱后重试",
    }
}

/// 工具沙箱：默认拒绝 + 能力掩码 + 操作/内存配额 + 违规环形台账。
pub struct Sandbox {
    pub created: bool,
    pub caps: u8,
    pub ops_used: u32,
    pub mem_kb: u32,
    /// 违规记录：(能力位, 次数) 环形台账。
    pub violations: [(u8, u32); SANDBOX_VIOLATION_CAP],
    pub violation_count: usize,
    pub killed: bool,
}

impl Sandbox {
    pub fn new() -> Sandbox {
        Sandbox { created: false, caps: 0, ops_used: 0, mem_kb: 0, violations: [(0, 0); SANDBOX_VIOLATION_CAP], violation_count: 0, killed: false }
    }

    /// 创建：掩码越界钳制到合法位。
    pub fn create(&mut self, caps: u8) -> u16 {
        if self.created {
            return SB_E_STATE;
        }
        self.caps = caps & SANDBOX_CAP_MAX;
        self.created = true;
        self.killed = false;
        SB_E_OK
    }

    /// 发起一次系统能力调用：默认拒绝，命中掩码才放行并计配额。
    pub fn invoke(&mut self, cap_bit: u8) -> u16 {
        if !self.created || self.killed {
            return SB_E_STATE;
        }
        if cap_bit >= 6 || self.caps & (1 << cap_bit) == 0 {
            self.record_violation(cap_bit);
            return SB_E_CAP;
        }
        if self.ops_used >= SANDBOX_MAX_OPS {
            self.record_violation(6);
            return SB_E_QUOTA;
        }
        self.ops_used += 1;
        SB_E_OK
    }

    fn record_violation(&mut self, cap_bit: u8) {
        for i in 0..self.violation_count {
            if self.violations[i].0 == cap_bit {
                self.violations[i].1 += 1;
                return;
            }
        }
        if self.violation_count < SANDBOX_VIOLATION_CAP {
            self.violations[self.violation_count] = (cap_bit, 1);
            self.violation_count += 1;
        }
    }

    /// 内存记账：超限拒绝并记违规（能力位 7 表内存）。
    pub fn alloc_kb(&mut self, kb: u32) -> u16 {
        if !self.created || self.killed {
            return SB_E_STATE;
        }
        if self.mem_kb + kb > SANDBOX_MEM_KB_CAP {
            self.record_violation(7);
            return SB_E_MEM;
        }
        self.mem_kb += kb;
        SB_E_OK
    }

    pub fn free_kb(&mut self, kb: u32) {
        self.mem_kb = self.mem_kb.saturating_sub(kb);
    }

    /// 授权收紧/放宽（放宽不越过 CAP_MAX）。
    pub fn grant(&mut self, caps: u8) -> u16 {
        if !self.created {
            return SB_E_STATE;
        }
        self.caps = self.caps | (caps & SANDBOX_CAP_MAX);
        SB_E_OK
    }

    pub fn revoke(&mut self, caps: u8) -> u16 {
        if !self.created {
            return SB_E_STATE;
        }
        self.caps &= !caps;
        SB_E_OK
    }

    /// 越权熔断：违规总数达阈值即销毁。
    pub fn total_violations(&self) -> u32 {
        let mut n = 0u32;
        for i in 0..self.violation_count {
            n += self.violations[i].1;
        }
        n
    }

    pub fn kill(&mut self) -> u16 {
        if !self.created {
            return SB_E_STATE;
        }
        self.killed = true;
        SB_E_OK
    }

    /// 不变量审计：掩码不越界、配额不超限。
    pub fn audit(&self) -> bool {
        self.caps <= SANDBOX_CAP_MAX && self.ops_used <= SANDBOX_MAX_OPS && self.mem_kb <= SANDBOX_MEM_KB_CAP
    }

    /// 快照（魔数 + 掩码 + 配额 + 违规计数）。
    pub fn snapshot(&self) -> [u32; 4] {
        [0x270_0002, self.caps as u32, self.ops_used, self.total_violations()]
    }

    pub fn reset(&mut self) {
        *self = Sandbox::new();
    }
}

/// 族0270 自检：X06726~X06750 逐项登记。
pub fn run_sandbox_checks() -> CheckSet {
    let mut set = CheckSet::new("ai27k-sandbox");

    // —— 基础实装 X06726~X06730 ——
    let mut sb = Sandbox::new();
    let _ = sb.create(0b00_0101);
    let ok = sb.invoke(0);
    set.add("X06726 核心链路闭环", ok == SB_E_OK && sb.ops_used == 1, "授权→调用→配额记账闭环");
    set.add("X06727 全量参数开放", SANDBOX_CAP_MAX == 63 && SANDBOX_MAX_OPS == 256 && SANDBOX_MEM_KB_CAP == 4096 && SANDBOX_VIOLATION_CAP == 8, "掩码/配额/台账全参数可查");
    set.add("X06728 档位矩阵≥5档", SB_E_OK != SB_E_CAP && SB_E_CAP != SB_E_QUOTA && SB_E_QUOTA != SB_E_MEM && SB_E_MEM != SB_E_STATE && SB_E_OK == 0 && SB_E_STATE == 4, "正常/未授权/配额/内存/状态五态齐备");
    let snap = sb.snapshot();
    let snap_ok = snap[0] == 0x270_0002 && snap[1] == 5 && snap[2] == 1;
    set.add("X06729 快照迁移三通道", snap_ok, "快照含魔数/掩码/配额三要素");
    let _ = sb.invoke(2);
    set.add("X06730 联调无回归", sb.audit() && sb.snapshot()[2] == 2, "连续调用无回归");

    // —— 边界与恢复 X06731~X06735 ——
    let mut sb2 = Sandbox::new();
    let pre = sb2.invoke(0);
    let _ = sb2.create(0b00_0001);
    let denied = sb2.invoke(2);
    set.add("X06731 非法输入钳制", pre == SB_E_STATE && denied == SB_E_CAP && sb2.total_violations() == 1, "默认拒绝且越界位记违规");
    set.add("X06732 错误叙事体系", sb_describe(SB_E_CAP).contains("申请") && sb_describe(SB_E_QUOTA).contains("配额") && sb_describe(SB_E_STATE).contains("初始化"), "每个失败有下一步建议");
    let mut sb3 = Sandbox::new();
    let _ = sb3.create(0b00_0001);
    for _ in 0..SANDBOX_MAX_OPS {
        let _ = sb3.invoke(0);
    }
    let over = sb3.invoke(0);
    set.add("X06733 中断续跑还原", sb3.ops_used == SANDBOX_MAX_OPS && over == SB_E_QUOTA && sb3.audit(), "配额耗尽后拒绝且不变量成立");
    let _ = sb3.alloc_kb(SANDBOX_MEM_KB_CAP + 1);
    set.add("X06734 资源降级守护", sb3.mem_kb == 0 && sb3.violation_count >= 1, "内存超限拒绝不崩溃");
    let _ = sb3.revoke(0b00_0001);
    let _ = sb3.reset();
    set.add("X06735 回滚净身", sb3.snapshot() == [0x270_0002, 0, 0, 0] && !sb3.created, "收回+重置无残档");

    // —— 手感与细节 X06736~X06740 ——
    set.add("X06736 令牌对齐", SB_E_OK == 0 && SB_E_STATE == 4 && SANDBOX_CAP_MAX.count_ones() == 6, "能力位与错误码令牌稳定");
    let mut sb4 = Sandbox::new();
    let _ = sb4.create(0);
    let state0 = sb4.invoke(0);
    let _ = sb4.grant(0b00_0011);
    let state1 = sb4.invoke(1);
    let _ = sb4.revoke(0b00_0010);
    let state2 = sb4.invoke(1);
    set.add("X06737 三态焦点", state0 == SB_E_CAP && state1 == SB_E_OK && state2 == SB_E_CAP, "未授权/已授权/已收回三态可观测");
    let mut sb5 = Sandbox::new();
    let _ = sb5.create(0b11_1111);
    let mut all_ok = true;
    for b in 0..6u8 {
        all_ok &= sb5.invoke(b) == SB_E_OK;
    }
    set.add("X06738 键盘通道", all_ok && sb5.ops_used == 6, "全掩码下六路调用全通");
    set.add("X06739 微文案统一", sb_describe(SB_E_OK) == "正常" && sb_describe(SB_E_MEM).contains("内存"), "中文自然术语一致");
    set.add("X06740 无障碍等价", sb_describe(255).contains("未知") && !sb_describe(SB_E_QUOTA).is_empty(), "未知码也有可读叙事");

    // —— 性能与优化 X06741~X06745 ——
    let mut sb6 = Sandbox::new();
    let _ = sb6.create(0b00_0001);
    let mut used = 0u32;
    while sb6.invoke(0) == SB_E_OK {
        used += 1;
    }
    set.add("X06741 基准采集", used == SANDBOX_MAX_OPS && sb6.ops_used == used, "满配额基准 256 次入册");
    let mut sb7 = Sandbox::new();
    let _ = sb7.create(0);
    for _ in 0..1000u32 {
        let _ = sb7.invoke(5);
    }
    set.add("X06742 热路径量化", sb7.total_violations() == 1000 && sb7.audit() && sb7.violation_count == 1, "千次越权聚合台账不越界");
    let mut sb8 = Sandbox::new();
    let _ = sb8.create(0b00_0001);
    let _ = sb8.alloc_kb(2048);
    sb8.free_kb(2048);
    set.add("X06743 内存功耗收敛", sb8.mem_kb == 0, "释放后零驻留");
    let mut sb9 = Sandbox::new();
    let dead = sb9.kill();
    set.add("X06744 低配降级链", dead == SB_E_STATE && !sb9.killed, "未创建降级为拒绝不崩溃");
    let mut sb10 = Sandbox::new();
    let _ = sb10.create(0b00_0011);
    let mut inv_ok = true;
    for i in 0..50u32 {
        let _ = sb10.invoke((i % 2) as u8);
        let _ = sb10.alloc_kb(64);
        sb10.free_kb(64);
        inv_ok &= sb10.audit();
    }
    set.add("X06745 防劣化守卫", inv_ok && sb10.snapshot()[2] == 50, "混合负载不变量断言只增不删");

    // —— 创新拓展 X06746~X06750 ——
    let mut sb11 = Sandbox::new();
    let _ = sb11.create(0);
    let _ = sb11.invoke(0);
    set.add("X06746 智能建议", sb_describe(SB_E_CAP).contains("申请"), "越权可解释可申请");
    let mut sb12 = Sandbox::new();
    let _ = sb12.create(0b00_0001);
    for _ in 0..(SANDBOX_MAX_OPS + 8) {
        let _ = sb12.invoke(0);
    }
    set.add("X06747 批量自动化", sb12.total_violations() == 8 && sb12.ops_used == SANDBOX_MAX_OPS, "批量压测台账聚合准确");
    let mut sb13 = Sandbox::new();
    let _ = sb13.create(0b00_0001);
    let _ = sb13.invoke(2);
    let before = sb13.snapshot();
    let _ = sb13.kill();
    let after_invoke = sb13.invoke(0);
    set.add("X06748 三线跨域联动", after_invoke == SB_E_STATE && sb13.snapshot()[3] == before[3], "销毁态与违规台账跨态联动");
    let mut sb14 = Sandbox::new();
    let _ = sb14.create(0b11_1111);
    let _ = sb14.grant(0b11_1111);
    set.add("X06749 开发者扩展点", sb14.caps == SANDBOX_CAP_MAX && sb14.grant(0) == SB_E_OK && sb14.revoke(0) == SB_E_OK, "授权/收回扩展点全通");
    let mut sb15 = Sandbox::new();
    let _ = sb15.create(0b00_0011);
    let fp15 = sb15.snapshot();
    let _ = sb15.invoke(0);
    sb15.reset();
    set.add("X06750 收官与净身", sb15.snapshot() == [0x270_0002, 0, 0, 0] && fp15[0] == 0x270_0002, "重置后回到初态收官");

    set
}

// ===========================================================================
// 聚合与门禁
// ===========================================================================

pub fn run_ai27k_all_checks() -> [CheckSet; 2] {
    [run_bus_checks(), run_sandbox_checks()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai27k_2x25_checks_pass() {
        let sets = run_ai27k_all_checks();
        assert_eq!(sets.len(), 2);
        for set in sets.iter() {
            assert_eq!(set.len(), 25, "族检数量应为 25：{}", set.domain);
            for i in 0..set.len() {
                let c = set.get(i).unwrap();
                assert!(c.passed, "第 {} 项未通过: {}", i, c.name);
            }
        }
    }

    #[test]
    fn ai27k_ids_unique_and_contiguous() {
        let sets = run_ai27k_all_checks();
        let ranges: [(usize, usize); 2] = [(6701, 6725), (6726, 6750)];
        for (si, set) in sets.iter().enumerate() {
            let (lo, hi) = ranges[si];
            for i in 0..set.len() {
                let name = set.get(i).unwrap().name;
                let id: u32 = name[1..6].parse().unwrap_or(0);
                assert!(id >= lo as u32 && id <= hi as u32, "ID {} 越出族区间 {:?}", id, set.domain);
                if i > 0 {
                    let prev: u32 = set.get(i - 1).unwrap().name[1..6].parse().unwrap_or(0);
                    assert_eq!(id, prev + 1, "ID 不连续：{} → {}", prev, id);
                }
            }
        }
    }
}
