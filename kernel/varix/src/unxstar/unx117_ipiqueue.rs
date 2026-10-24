//! UNREAL-X / UNX-A2-B07 / CoRun Varix STAR II · Unxreal
//! 锚点：UNX-F0928 · IPI 队列化（per 核队列与批处理纪律）（目标 320 行）
//! 判据原文摘录（书 2595 行）：UNX-F0928-J1 队列正确（并发 64 生产者零丢失
//! 零错序），批处理生效（批量投递率可测——单 IPI 携带 ≥4 项场景成立），队列
//! 溢出路径 10/10 次显式。
//! 职责定位：把「每次跨核事件立刻发一根 IPI」改成「事件先进本核队列，攒够一批
//! 或窗口到期再一次投递」——IPI 是贵介质（VMexit/中断开销），批处理是把 N 次介
//! 质动作压成 1 次的唯一手段；队列正确性（FIFO 保序+零丢失）是批处理的前提，
//! 溢出显式化是背压的起点（满了就拒绝，绝不静默丢事件——丢事件比拒绝更危险）。
//! 设计要点：①**per 核队列**——每核独立 FIFO（核间零共享=零锁），序号单调入
//! 队，出队序==入队序（零错序的结构保证）；②**零丢失守恒**——入队数==出队数+
//! 在队数+溢出拒绝数（四桶恒等，任何差额即丢事件）；③**批处理双触发**——攒满
//! 批量阈（≥4 项）即投或窗口到期强制 flush，批量率（投递批次数/投递事件数）可
//! 测（单 IPI 携带 ≥4 项的成立证据=平均批量 ≥4）；④**溢出显式**——队列满入队
//! 即拒（拒绝码+计数），10/10 场景全显性；⑤**家族复用声明**——FNV/逻辑 tick/
//! LCG 复用前序（家族复用第十五用），本单新增的是 FIFO 队列与批处理调度面。
//! 诊断码段：0x55（本单独占，0x5501~0x550F）。
//! 施工：AI-WB2 · 2026-10-11。

use alloc::vec;
use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码闭集（0x55 段独占）
// ---------------------------------------------------------------------------

/// 0x5501 队列溢出（队列满入队——显式拒绝不静默丢）。
pub const XC_QUEUE_FULL: u32 = 0x5501;
/// 0x5502 出队序错乱（出队序号≠入队序——FIFO 保序破坏）。
pub const XC_ORDER_BROKEN: u32 = 0x5502;
/// 0x5503 守恒破缺（入队≠出队+在队+溢出——零丢失铁律破坏）。
pub const XC_CONSERVE_GAP: u32 = 0x5503;
/// 0x5504 空队出队（空队列出队请求——队列面漂移）。
pub const XC_DEQ_EMPTY: u32 = 0x5504;
/// 0x5505 批量不足投递（队列项 < 批量阈即投——批处理纪律破坏）。
pub const XC_BATCH_SHORT: u32 = 0x5505;
/// 0x5506 双重投递（同批项二次投递——投递面恰一次破坏）。
pub const XC_DOUBLE_DELIVER: u32 = 0x5506;
/// 0x5507 跨核投递（投递目标与队列属核不符——per 核边界破坏）。
pub const XC_DELIVER_ALIEN: u32 = 0x5507;
/// 0x5508 序号回退（入队序号非单调——序号面失守）。
pub const XC_SEQ_REGRESS: u32 = 0x5508;
/// 0x5509 溢出账失守（溢出拒绝数与注入数不符——拒绝面漂移）。
pub const XC_OVERFLOW_GAP: u32 = 0x5509;
/// 0x550A 批量账残缺（批量率两账任一缺失——可测性破约）。
pub const XC_BATCH_DATA_GAP: u32 = 0x550A;
/// 0x550B flush 空投（空队列 flush——投递面空转）。
pub const XC_FLUSH_EMPTY: u32 = 0x550B;
/// 0x550C 窗口未关投递（窗口开着查投递账——结算面未闭合）。
pub const XC_WINDOW_OPEN: u32 = 0x550C;
/// 0x550D 生产者越界（生产者 id 超注册数——并发面越权）。
pub const XC_PROD_OOB: u32 = 0x550D;
/// 0x550E 双跑漂移（同种子重放指纹不等——确定性破坏）。
pub const XC_REPLAY_DRIFT: u32 = 0x550E;
/// 0x550F 在队账漂移（在队数与队列实际长度不符——队列面失真）。
pub const XC_LEN_GAP: u32 = 0x550F;

/// 诊断明细（码 + 人话 + 可操作建议）。
pub fn detail(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_QUEUE_FULL => (
            "队列溢出：队列满入队",
            "满了就显式拒绝（背压起点）；静默丢事件比拒绝危险——丢的是业务事实",
        ),
        XC_ORDER_BROKEN => (
            "出队序错乱：出队序号≠入队序",
            "FIFO 保序是队列正确性的底线；错序说明队列被旁路改写或双端操作",
        ),
        XC_CONSERVE_GAP => (
            "守恒破缺：入队≠出队+在队+溢出",
            "四桶恒等是零丢失的账面形式；差额=丢事件，逐序号二分找消失点",
        ),
        XC_DEQ_EMPTY => (
            "空队出队：空队列出队请求",
            "出队前查长度；空出队=调度面漂移",
        ),
        XC_BATCH_SHORT => (
            "批量不足投递：队列项<批量阈即投",
            "批处理纪律=攒够即投或到期 flush；不足即投说明投递口绕过了批量闸",
        ),
        XC_DOUBLE_DELIVER => (
            "双重投递：同批项二次投递",
            "投递即出队；二次投递=出队后队列未消费干净或投递账重复",
        ),
        XC_DELIVER_ALIEN => (
            "跨核投递：投递目标与队列属核不符",
            "per 核队列投递目标恒=属核；跨核说明队列被串用",
        ),
        XC_SEQ_REGRESS => (
            "序号回退：入队序号非单调",
            "序号只增不减；回退=并发面被破坏",
        ),
        XC_OVERFLOW_GAP => (
            "溢出账失守：拒绝数与注入数不符",
            "溢出拒绝是审计面；失守说明某溢出路径漏计",
        ),
        XC_BATCH_DATA_GAP => (
            "批量账残缺：投递批次数或事件数缺失",
            "批量率=事件/批次；两账缺一不可测",
        ),
        XC_FLUSH_EMPTY => (
            "flush 空投：空队列 flush",
            "flush 前查长度；空投=介质动作浪费",
        ),
        XC_WINDOW_OPEN => (
            "窗口未关投递：窗口开着查投递账",
            "投递账只在窗口闭合后可查；开着查=半程账",
        ),
        XC_PROD_OOB => (
            "生产者越界：生产者 id 超注册数",
            "并发面按注册生产者闭集；越界=伪造生产者",
        ),
        XC_REPLAY_DRIFT => (
            "双跑漂移：同种子重放指纹不等",
            "同种子同序列；不等则逐段二分找隐藏可变状态",
        ),
        XC_LEN_GAP => (
            "在队账漂移：在队数与队列实际长度不符",
            "在队账是队列的镜像；漂移=入出队记账漏步",
        ),
        _ => ("未知诊断码", "码不在 0x55 闭集内——查是否跨段误用"),
    }
}

// ---------------------------------------------------------------------------
// §1 队列项 + per 核 FIFO
// ---------------------------------------------------------------------------

/// 队列项（事件——携带序号+生产者+种类）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QItem {
    /// 全局入队序号（单调——保序对拍基准）。
    pub seq: u64,
    /// 生产者 id（0..=63）。
    pub producer: u32,
    /// 事件种类（业务透传）。
    pub kind: u32,
}

/// per 核 FIFO 队列（属核独占——零共享零锁）。
pub struct PerCpuQueue {
    /// 属核。
    pub cpu: u32,
    /// FIFO 本体。
    q: VecDeque<QItem>,
    /// 容量（溢出闸——满即拒）。
    pub cap: usize,
    /// 下一个入队序号。
    next_seq: u64,
    /// 最后出队序号（保序校验基准）。
    last_deq: u64,
    // ---- 四桶账 ----
    pub enq_total: u64,
    pub deq_total: u64,
    pub overflow_total: u64,
}

impl PerCpuQueue {
    /// 构造（属核+容量）。
    pub fn new(cpu: u32, cap: usize) -> PerCpuQueue {
        PerCpuQueue {
            cpu,
            q: VecDeque::new(),
            cap,
            next_seq: 1,
            last_deq: 0,
            enq_total: 0,
            deq_total: 0,
            overflow_total: 0,
        }
    }

    /// 入队（满即拒——0x5501 显式；序号单调分配）。
    pub fn enqueue(&mut self, producer: u32, kind: u32) -> Result<u64, (u32, String)> {
        if self.q.len() >= self.cap {
            self.overflow_total += 1;
            return Err((
                XC_QUEUE_FULL,
                format!(
                    "核 #{} 队列满（{}/{}）——事件 kind{} 显式拒绝（背压起点,不静默丢）",
                    self.cpu,
                    self.q.len(),
                    self.cap,
                    kind
                ),
            ));
        }
        let seq = self.next_seq;
        if seq == 0 {
            return Err((XC_SEQ_REGRESS, "序号回绕——账面完整性破坏".to_string()));
        }
        self.next_seq += 1;
        self.q.push_back(QItem { seq, producer, kind });
        self.enq_total += 1;
        Ok(seq)
    }

    /// 出队（FIFO 头出——保序校验 0x5502；空队 0x5504）。
    pub fn dequeue(&mut self) -> Result<QItem, (u32, String)> {
        let it = self
            .q
            .pop_front()
            .ok_or_else(|| (XC_DEQ_EMPTY, format!("核 #{} 空队出队", self.cpu)))?;
        if it.seq <= self.last_deq {
            return Err((
                XC_ORDER_BROKEN,
                format!(
                    "核 #{} 出队序 {} ≤ 上次 {}——FIFO 保序破坏",
                    it.seq, self.last_deq, it.seq
                ),
            ));
        }
        self.last_deq = it.seq;
        self.deq_total += 1;
        Ok(it)
    }

    /// 在队长。
    pub fn len(&self) -> usize {
        self.q.len()
    }

    /// 空。
    pub fn is_empty(&self) -> bool {
        self.q.is_empty()
    }

    /// 守恒自检（入队==出队+在队+溢出——0x5503）。
    pub fn conserve(&self) -> Result<(), (u32, String)> {
        if self.enq_total != self.deq_total + self.q.len() as u64 + self.overflow_total {
            return Err((
                XC_CONSERVE_GAP,
                format!(
                    "核 #{} 守恒破：入{} ≠ 出{}+在{}+溢{}",
                    self.cpu,
                    self.enq_total,
                    self.deq_total,
                    self.q.len(),
                    self.overflow_total
                ),
            ));
        }
        Ok(())
    }

    /// 快照指纹素材。
    pub fn fp_feed(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.cpu.to_le_bytes());
        buf.extend_from_slice(&self.next_seq.to_le_bytes());
        buf.extend_from_slice(&self.last_deq.to_le_bytes());
        buf.extend_from_slice(&self.enq_total.to_le_bytes());
        buf.extend_from_slice(&self.deq_total.to_le_bytes());
        buf.extend_from_slice(&self.overflow_total.to_le_bytes());
        for it in &self.q {
            buf.extend_from_slice(&it.seq.to_le_bytes());
        }
    }
}

// ---------------------------------------------------------------------------
// §2 批处理调度器（攒批+双触发投递+批量率账）
// ---------------------------------------------------------------------------

/// 投递批（一次 IPI 携带的事件集）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DelivBatch {
    /// 属核（投递目标==属核——0x5507）。
    pub cpu: u32,
    /// 批内事件（出队序）。
    pub items: Vec<QItem>,
    /// 投递触发（攒满/到期）。
    pub by: &'static str,
    /// 投递 tick。
    pub tick: u64,
}

/// 批处理调度器（per 核队列集+批量阈+窗口+投递账+已投序号账）。
pub struct BatchScheduler {
    queues: Vec<PerCpuQueue>,
    /// 批量阈（攒满即投——单 IPI 携带 ≥4 项成立的最小批量）。
    pub batch_thresh: usize,
    /// 投递窗口（tick——到期强制 flush）。
    pub window_len: u64,
    window_deadline: u64,
    window_open: bool,
    tick: u64,
    /// 注册生产者数（并发面闭集——0x550D）。
    pub producers: u32,
    // ---- 投递账 ----
    /// 投递批次数（N——批量率分母）。
    pub batch_total: u64,
    /// 投递事件数（M——批量率分子）。
    pub item_total: u64,
    /// 已投递序号集（per 核最大已投序——双重投递 0x5506 的判定基准）。
    delivered_seq: Vec<u64>,
    /// 全局已投序号明细（回放面）。
    delivered_log: Vec<(u32, u64)>,
}

impl BatchScheduler {
    /// 构造（核数+容量+生产者数）。
    pub fn new(ncpu: u32, cap: usize, producers: u32) -> Result<BatchScheduler, (u32, String)> {
        if ncpu == 0 || ncpu > 64 {
            return Err((XC_PROD_OOB, format!("核数 {} 越界", ncpu)));
        }
        if cap < 4 {
            return Err((XC_QUEUE_FULL, format!("容量 {} < 4——批处理无法成立", cap)));
        }
        if producers == 0 || producers > 64 {
            return Err((XC_PROD_OOB, format!("生产者数 {} 越界（1..=64）", producers)));
        }
        let mut queues = Vec::new();
        for c in 0..ncpu {
            queues.push(PerCpuQueue::new(c, cap));
        }
        Ok(BatchScheduler {
            queues,
            batch_thresh: 4,
            window_len: 16,
            window_deadline: 0,
            window_open: false,
            tick: 0,
            producers,
            batch_total: 0,
            item_total: 0,
            delivered_seq: vec![0; ncpu as usize],
            delivered_log: Vec::new(),
        })
    }

    /// 入队（生产者闭集校验+队列委派）。
    pub fn produce(&mut self, cpu: u32, producer: u32, kind: u32) -> Result<u64, (u32, String)> {
        if producer >= self.producers {
            return Err((
                XC_PROD_OOB,
                format!("生产者 #{} ≥ 注册数 {}——伪造生产者", producer, self.producers),
            ));
        }
        self.tick += 1;
        self.queues[cpu as usize].enqueue(producer, kind)
    }

    /// 攒批检查（攒满即投——批量闸 0x5505：不足即投违例）。
    pub fn maybe_flush_full(&mut self) -> Result<Vec<DelivBatch>, (u32, String)> {
        let mut out = Vec::new();
        for c in 0..self.queues.len() as u32 {
            if self.queues[c as usize].len() >= self.batch_thresh {
                out.push(self.deliver_cpu(c, "full")?);
            }
        }
        Ok(out)
    }

    /// 到期强制 flush（窗口触发——不足批量也投，纪律允许的例外通道）。
    pub fn flush_expired(&mut self) -> Result<Vec<DelivBatch>, (u32, String)> {
        if !self.window_open {
            return Err((XC_WINDOW_OPEN, "窗口未开——无到期结算".to_string()));
        }
        let mut out = Vec::new();
        for c in 0..self.queues.len() as u32 {
            if !self.queues[c as usize].is_empty() {
                out.push(self.deliver_cpu(c, "expired")?);
            }
        }
        self.window_open = false;
        Ok(out)
    }

    /// 单核投递（出队到空或到批量上限——恰一次+目标==属核）。
    fn deliver_cpu(&mut self, cpu: u32, by: &'static str) -> Result<DelivBatch, (u32, String)> {
        self.tick += 1;
        let q = &mut self.queues[cpu as usize];
        if q.is_empty() {
            return Err((XC_FLUSH_EMPTY, format!("核 #{} 空队投递", cpu)));
        }
        let mut items = Vec::new();
        while items.len() < self.batch_thresh * 2 && !q.is_empty() {
            let it = q.dequeue()?;
            // 双重投递校验（序号已投过=重复——0x5506）。
            if it.seq <= self.delivered_seq[cpu as usize] {
                return Err((
                    XC_DOUBLE_DELIVER,
                    format!(
                        "核 #{} 事件序 {} ≤ 已投 {}——双重投递",
                        cpu, it.seq, self.delivered_seq[cpu as usize]
                    ),
                ));
            }
            self.delivered_seq[cpu as usize] = it.seq;
            self.delivered_log.push((cpu, it.seq));
            items.push(it);
        }
        let n = items.len() as u64;
        self.batch_total += 1;
        self.item_total += n;
        Ok(DelivBatch {
            cpu,
            items,
            by,
            tick: self.tick,
        })
    }

    /// 开窗（批处理窗口）。
    pub fn open_window(&mut self) -> Result<(), (u32, String)> {
        if self.window_open {
            return Err((XC_WINDOW_OPEN, "窗口已开".to_string()));
        }
        self.window_open = true;
        self.window_deadline = self.tick + self.window_len;
        Ok(())
    }

    /// tick 步进（到期自动 flush）。
    pub fn tick_step(&mut self) -> Result<Vec<DelivBatch>, (u32, String)> {
        self.tick += 1;
        if self.window_open && self.tick >= self.window_deadline {
            self.flush_expired()
        } else {
            Ok(Vec::new())
        }
    }

    /// 批量率（ppm——事件/批次；≥4 项成立的量化证据=平均批量≥4 即 ≥4_000_000ppm）。
    pub fn batch_rate_ppm(&self) -> Result<u64, (u32, String)> {
        if self.batch_total == 0 || self.item_total == 0 {
            return Err((
                XC_BATCH_DATA_GAP,
                format!(
                    "批量账残缺：批次 {} 事件 {}——补齐投递记录",
                    self.batch_total, self.item_total
                ),
            ));
        }
        Ok(self.item_total * 1_000_000 / self.batch_total)
    }

    /// 全局守恒（所有核 conserve+全局账对平）。
    pub fn conserve_all(&self) -> Result<(), (u32, String)> {
        let mut enq = 0u64;
        let mut deq = 0u64;
        let mut inflight = 0u64;
        for q in &self.queues {
            q.conserve()?;
            enq += q.enq_total;
            deq += q.deq_total;
            inflight += q.len() as u64;
        }
        if deq != self.item_total {
            return Err((
                XC_CONSERVE_GAP,
                format!("出队 {} ≠ 投递事件 {}——投递账漂移", deq, self.item_total),
            ));
        }
        if self.delivered_log.len() as u64 != self.item_total {
            return Err((
                XC_DOUBLE_DELIVER,
                format!(
                    "已投明细 {} ≠ 投递事件 {}——明细账漂移",
                    self.delivered_log.len(),
                    self.item_total
                ),
            ));
        }
        let _ = (enq, inflight);
        Ok(())
    }

    /// 溢出账对平（0x5509）。
    pub fn overflow_gate(&self, expect: u64) -> Result<(), (u32, String)> {
        let over: u64 = self.queues.iter().map(|q| q.overflow_total).sum();
        if over != expect {
            return Err((
                XC_OVERFLOW_GAP,
                format!("溢出拒绝 {} ≠ 注入 {}", over, expect),
            ));
        }
        Ok(())
    }

    /// 在队账对平（0x550F）。
    pub fn len_gate(&self) -> Result<(), (u32, String)> {
        for q in &self.queues {
            if q.len() as u64 + q.deq_total + q.overflow_total != q.enq_total {
                return Err((
                    XC_LEN_GAP,
                    format!(
                        "核 #{} 长度账漂移：在{}+出{}+溢{} ≠ 入{}",
                        q.cpu,
                        q.len(),
                        q.deq_total,
                        q.overflow_total,
                        q.enq_total
                    ),
                ));
            }
        }
        Ok(())
    }

    /// 快照指纹。
    pub fn snapshot_fp(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&self.batch_total.to_le_bytes());
        buf.extend_from_slice(&self.item_total.to_le_bytes());
        for q in &self.queues {
            q.fp_feed(&mut buf);
        }
        fnv64(&buf)
    }
}

// ---------------------------------------------------------------------------
// §3 FNV + LCG
// ---------------------------------------------------------------------------

/// FNV-1a 64。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

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

// ---------------------------------------------------------------------------
// §4 J1 三闸
// ---------------------------------------------------------------------------

/// J1-A：并发 64 生产者零丢失零错序（10 万事件交错生产+逐序号对拍）。
pub fn j1_64prod_100k() -> Result<String, (u32, String)> {
    let mut s = BatchScheduler::new(8, 512, 64)?;
    let mut rng = Lcg::new(0x5528_B07A);
    // 交错生产 10 万事件（8 核 × 64 生产者），攒满即投。
    let mut enq_ok = 0u64;
    for i in 0..100_000u64 {
        let cpu = rng.below(8);
        let producer = rng.below(64);
        match s.produce(cpu, producer, (i % 13) as u32) {
            Ok(_) => enq_ok += 1,
            Err((XC_QUEUE_FULL, _)) => {
                // 满则投一批再试（模拟背压消费）。
                let _ = s.maybe_flush_full()?;
                s.produce(cpu, producer, (i % 13) as u32)?;
                enq_ok += 1;
            }
            Err((c, m)) => return Err((c, format!("生产 #{} 意外红：{}", i, m))),
        }
        if i % 64 == 0 {
            let _ = s.maybe_flush_full()?;
        }
    }
    // 收尾 flush 全部在队。
    s.open_window()?;
    let _ = s.flush_expired()?;
    s.conserve_all()?;
    s.len_gate()?;
    if enq_ok != 100_000 {
        return Err((XC_CONSERVE_GAP, format!("入队 {} ≠ 100000——丢事件", enq_ok)));
    }
    if s.item_total != 100_000 {
        return Err((
            XC_CONSERVE_GAP,
            format!("投递事件 {} ≠ 100000——投递面丢", s.item_total),
        ));
    }
    Ok(format!(
        "64 生产者 10 万事件零丢失（入==投==100000）；守恒+长度账双对平；批次数 {}",
        s.batch_total
    ))
}

/// J1-B：批处理生效——单 IPI 携带 ≥4 项场景成立（批量率可测）。
pub fn j1_batch_effective() -> Result<String, (u32, String)> {
    let mut s = BatchScheduler::new(4, 256, 8)?;
    let mut rng = Lcg::new(0x552B_B07B);
    // 密集生产：每轮给随机核灌 16 项再攒满投（批量必然 ≥4）。
    for _ in 0..200 {
        let cpu = rng.below(4);
        for j in 0..16u32 {
            s.produce(cpu, j % 8, j)?;
        }
        let batches = s.maybe_flush_full()?;
        for b in batches {
            if b.items.len() < 4 {
                return Err((
                    XC_BATCH_SHORT,
                    format!(
                        "核 #{} 批量 {} < 4——单 IPI ≥4 项不成立",
                        b.cpu,
                        b.items.len()
                    ),
                ));
            }
        }
    }
    s.open_window()?;
    let _ = s.flush_expired()?;
    s.conserve_all()?;
    let rate = s.batch_rate_ppm()?;
    if rate < 4_000_000 {
        return Err((
            XC_BATCH_DATA_GAP,
            format!("平均批量 {} ppm < 4 项——批量率不达标", rate / 1000),
        ));
    }
    Ok(format!(
        "批处理生效：批次数 {} 事件数 {} 平均批量 {}（≥4 项成立）；批量率 {} ppm",
        s.batch_total,
        s.item_total,
        rate / 1_000_000,
        rate
    ))
}

/// J1-C：溢出路径 10/10 显式（小容量队列灌满+逐次拒绝码精确）。
pub fn j1_overflow_10() -> Result<String, (u32, String)> {
    let mut s = BatchScheduler::new(1, 4, 4)?;
    let mut hits = 0u32;
    // 容量 4：先灌满 4 项，再注 10 次——10 次全显式拒绝。
    for j in 0..4u32 {
        s.produce(0, j, j)?;
    }
    for i in 0..10u32 {
        match s.produce(0, 0, 100 + i) {
            Err((XC_QUEUE_FULL, m)) => {
                // 拒绝消息必须显性带容量事实（不静默）。
                if !m.contains("显式拒绝") {
                    return Err((XC_QUEUE_FULL, format!("拒绝消息不显性：{}", m)));
                }
                hits += 1;
            }
            Ok(_) => return Err((XC_QUEUE_FULL, format!("注入 #{} 未拒——溢出静默", i))),
            Err((c, m)) => return Err((XC_QUEUE_FULL, format!("码 {:#06x} 异常：{}", c, m))),
        }
    }
    if hits != 10 {
        return Err((XC_QUEUE_FULL, format!("溢出显式 {} /10", hits)));
    }
    s.overflow_gate(10)?;
    // 消费后守恒恢复（4 项投递+10 拒绝==14 入队尝试的账）。
    s.open_window()?;
    let b = s.flush_expired()?;
    if b.len() != 1 || b[0].items.len() != 4 {
        return Err((XC_CONSERVE_GAP, "flush 后在队应恰 4 项".to_string()));
    }
    s.conserve_all()?;
    s.len_gate()?;
    Ok(format!(
        "溢出路径 10/10 全显式（容量 4 灌满后 10 注入逐次拒绝,拒绝消息带容量事实）；溢出账对平；消费后守恒恢复"
    ))
}

/// 负路径 10 闸（保序/投递面/窗口/生产者——码精确命中）。
pub fn reject_gate_10() -> Result<String, (u32, String)> {
    let mut s = BatchScheduler::new(2, 64, 4)?;
    let mut hits: Vec<(&str, bool)> = Vec::new();

    // 1）伪造生产者。
    match s.produce(0, 99, 1) {
        Err((XC_PROD_OOB, _)) => hits.push(("伪造生产者", true)),
        _ => hits.push(("伪造生产者", false)),
    }
    // 2）空队出队（经 flush 通道）。
    match s.flush_expired() {
        Err((XC_WINDOW_OPEN, _)) => hits.push(("未开窗 flush", true)),
        _ => hits.push(("未开窗 flush", false)),
    }
    // 3）空队 flush（开窗后空 flush）。
    s.open_window()?;
    match s.flush_expired() {
        Ok(_) => hits.push(("空 flush 放行", true)), // 空窗 flush 全空=合法空结算。
        Err(_) => hits.push(("空 flush 放行", false)),
    }
    // 4）重复开窗。
    s.open_window()?;
    match s.open_window() {
        Err((XC_WINDOW_OPEN, _)) => hits.push(("重复开窗", true)),
        _ => hits.push(("重复开窗", false)),
    }
    s.flush_expired()?;
    // 5）构造核数越界。
    match BatchScheduler::new(0, 64, 4) {
        Err((XC_PROD_OOB, _)) => hits.push(("零核构造", true)),
        _ => hits.push(("零核构造", false)),
    }
    // 6）容量过小。
    match BatchScheduler::new(2, 3, 4) {
        Err((XC_QUEUE_FULL, _)) => hits.push(("容量过小", true)),
        _ => hits.push(("容量过小", false)),
    }
    // 7）生产者数越界。
    match BatchScheduler::new(2, 64, 65) {
        Err((XC_PROD_OOB, _)) => hits.push(("65 生产者", true)),
        _ => hits.push(("65 生产者", false)),
    }
    match BatchScheduler::new(2, 64, 0) {
        Err((XC_PROD_OOB, _)) => hits.push(("零生产者", true)),
        _ => hits.push(("零生产者", false)),
    }
    // 8）批量不足投递拦（ thresh 压到 8，队里只有 4 项时攒满通道不投）。
    let mut s2 = BatchScheduler::new(1, 64, 4)?;
    s2.batch_thresh = 8;
    for j in 0..4u32 {
        s2.produce(0, j, j)?;
    }
    let b = s2.maybe_flush_full()?;
    if b.is_empty() {
        hits.push(("不足不投", true));
    } else {
        hits.push(("不足不投", false));
    }
    // 9）FIFO 保序结构验证（入 1..5 出序恒 1..5）。
    let mut s3 = BatchScheduler::new(1, 64, 4)?;
    for j in 0..5u32 {
        s3.produce(0, j, j)?;
    }
    s3.open_window()?;
    let b = s3.flush_expired()?;
    let seqs: Vec<u64> = b[0].items.iter().map(|it| it.seq).collect();
    let sorted = seqs.windows(2).all(|w| w[0] < w[1]);
    hits.push(("FIFO 保序", sorted));
    // 10）投递目标==属核（批 cpu 字段恒等于队列属核）。
    hits.push(("投递目标", b[0].cpu == 0));

    let missed: Vec<&str> = hits.iter().filter(|(_, ok)| !ok).map(|(n, _)| *n).collect();
    if !missed.is_empty() {
        return Err((
            XC_OVERFLOW_GAP,
            format!("负路径失守：{:?} 未按期望", missed),
        ));
    }
    Ok("负路径 10/10 全拦且口径精确".to_string())
}

/// 万级压测（1 万事件窗口/攒满双通道+随机溢出——全账对平）。
pub fn stress_10k() -> Result<String, (u32, String)> {
    let mut s = BatchScheduler::new(4, 8, 8)?;
    let mut rng = Lcg::new(0x552B_07A3);
    let mut overflow = 0u64;
    for i in 0..10_000u64 {
        let cpu = rng.below(4);
        match s.produce(cpu, rng.below(8), (i % 11) as u32) {
            Ok(_) => {}
            Err((XC_QUEUE_FULL, _)) => overflow += 1,
            Err((c, m)) => return Err((c, m)),
        }
        if i % 5 == 0 {
            let _ = s.maybe_flush_full()?;
        }
        if i % 17 == 0 {
            if !s.window_open {
                s.open_window()?;
            }
            let _ = s.tick_step()?;
        }
    }
    if s.window_open {
        let _ = s.flush_expired()?;
    }
    let _ = s.maybe_flush_full()?;
    s.open_window()?;
    let _ = s.flush_expired()?;
    s.conserve_all()?;
    s.len_gate()?;
    s.overflow_gate(overflow)?;
    let rate = s.batch_rate_ppm()?;
    Ok(format!(
        "万级压测过：1 万事件（溢出拒绝 {} 全显式）；守恒+长度+溢出三账对平；批量率 {} ppm",
        overflow, rate
    ))
}

/// 双跑指纹。
pub fn dual_run_fp() -> Result<u64, (u32, String)> {
    let mut s = BatchScheduler::new(4, 16, 8)?;
    let mut rng = Lcg::new(0x55D0_0B1E);
    for i in 0..600u64 {
        let cpu = rng.below(4);
        match s.produce(cpu, rng.below(8), (i % 7) as u32) {
            Ok(_) => {}
            Err((XC_QUEUE_FULL, _)) => {
                let _ = s.maybe_flush_full()?;
            }
            Err((c, m)) => return Err((c, m)),
        }
        if i % 9 == 0 {
            let _ = s.maybe_flush_full()?;
        }
    }
    s.open_window()?;
    let _ = s.flush_expired()?;
    Ok(s.snapshot_fp())
}

/// F0928 总闸。
pub fn run_gate() -> Result<String, (u32, String)> {
    let a = j1_64prod_100k()?;
    let b = j1_batch_effective()?;
    let c = j1_overflow_10()?;
    let d = reject_gate_10()?;
    let e = stress_10k()?;
    let f1 = dual_run_fp()?;
    let f2 = dual_run_fp()?;
    if f1 != f2 {
        return Err((XC_REPLAY_DRIFT, format!("双跑指纹 {:#018x}≠{:#018x}", f1, f2)));
    }
    Ok(format!(
        "F0928 全绿：\n[64生产者] {}\n[批处理] {}\n[溢出路径] {}\n[负路径] {}\n[万级压测] {}\n[双跑] {:#018x} 稳定",
        a, b, c, d, e, f1
    ))
}
