//! VE-F0202 续 · 队列管理 + 在途跟踪 + 超时纪律
//!
//! 判据映射：
//! - 在途配对零串扰 → `Inflight`（fence↔命令配对；未知 fence 应答 =
//!   串扰，显性拒绝留痕）；
//! - 超时联动丢失状态机 → `poll()`（每命令默认 2 秒，超时产出
//!   `DeviceSuspicion` 记录，交 F0009 设备丢失状态机消费）；
//! - avail/used 环操作 → `AvailRing`/`UsedRing`（描述符链组装、索引推进、
//!   used 收割——中断驱动的收割在本模型里是显式 `harvest` 调用）。
//!
//! 确定性纪律：设备应答由对拍设备（`veb02_proto::ReferenceDevice`）注入，
//! 时间用逻辑 tick（µs）注入，无墙钟、零 IO，回归可复现。

use super::veb02_proto::*;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、描述符与 avail/used 环
// ---------------------------------------------------------------------------

/// 描述符标志位。
pub const DESC_F_NEXT: u16 = 0x1;
pub const DESC_F_WRITE: u16 = 0x2;

/// 单条描述符上限——长命令拆链（决定链长的是长度不是猜）。
pub const DESC_MAX_LEN: usize = 512;

/// 描述符表条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Descriptor {
    /// 驱动侧缓冲地址（本模型用 wire 缓冲的起始序号代替真实地址，
    /// 语义与规范一致：设备只搬字节不解释地址）
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}

/// avail 环：驱动 → 设备 的可用描述符链登记。
#[derive(Clone, Debug, Default)]
pub struct AvailRing {
    pub flags: u16,
    /// 环索引（规范语义：单调递增，模环长取槽位）
    pub idx: u16,
    pub ring: Vec<u16>,
}

/// used 环条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsedElem {
    /// 链头描述符索引
    pub id: u32,
    /// 设备写入字节数
    pub len: u32,
}

/// used 环：设备 → 驱动 的完成登记。
#[derive(Clone, Debug, Default)]
pub struct UsedRing {
    pub flags: u16,
    pub idx: u16,
    pub ring: Vec<UsedElem>,
}

/// 链组装失败（三要素）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChainError {
    pub code: &'static str,
    pub what: String,
    pub why: String,
    pub next: String,
}

/// 描述符表 + 链组装。空表拒绝、零长载荷拒绝（空链没有语义）。
#[derive(Clone, Debug)]
pub struct DescTable {
    descs: Vec<Descriptor>,
    used_flags: Vec<bool>,
    capacity: u16,
}

impl DescTable {
    pub fn new(capacity: u16) -> DescTable {
        DescTable {
            descs: Vec::new(),
            used_flags: Vec::new(),
            capacity,
        }
    }

    pub fn capacity(&self) -> u16 {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.descs.len()
    }

    /// 组装一条链：把 wire 缓冲拆成 ≤DESC_MAX_LEN 的描述符序列。
    ///
    /// 返回链头索引。容量耗尽 / 零长载荷显性拒绝（不静默截断）。
    pub fn push_chain(&mut self, wire_len: usize) -> Result<u16, ChainError> {
        if wire_len == 0 {
            return Err(ChainError {
                code: "E_CHAIN_EMPTY",
                what: "描述符链载荷长度为 0".to_string(),
                why: "空链在规范里没有语义，设备无从消费".to_string(),
                next: "提交前校验 wire 缓冲非空".to_string(),
            });
        }
        let chain_len = wire_len.div_ceil(DESC_MAX_LEN);
        if self.descs.len() + chain_len > self.capacity as usize {
            return Err(ChainError {
                code: "E_DESC_EXHAUSTED",
                what: format!(
                    "描述符表容量 {} 不足：已有 {} 条，本链还需 {} 条",
                    self.capacity,
                    self.descs.len(),
                    chain_len
                ),
                why: "描述符耗尽时继续提交会写穿表边界".to_string(),
                next: "先收割 used 环释放链，或扩容描述符表（2 的幂）".to_string(),
            });
        }
        let head = self.descs.len() as u16;
        let mut left = wire_len;
        for i in 0..chain_len {
            let seg = left.min(DESC_MAX_LEN);
            self.descs.push(Descriptor {
                addr: (head as u64) + i as u64,
                len: seg as u32,
                flags: if i + 1 < chain_len {
                    DESC_F_NEXT
                } else {
                    DESC_F_WRITE
                },
                next: if i + 1 < chain_len {
                    (head as usize + i + 1) as u16
                } else {
                    0
                },
            });
            self.used_flags.push(false);
            left -= seg;
        }
        Ok(head)
    }

    /// 释放一条链（收割后调用）。返回释放的条数。
    pub fn free_chain(&mut self, head: u16) -> usize {
        let mut i = head as usize;
        let mut n = 0;
        while i < self.descs.len() && self.used_flags[i] {
            self.used_flags[i] = false;
            let has_next = self.descs[i].flags & DESC_F_NEXT != 0
                && self.descs[i].next != 0
                && self.descs[i].next as usize > i;
            n += 1;
            if has_next {
                i = self.descs[i].next as usize;
            } else {
                break;
            }
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 二、在途跟踪与超时纪律
// ---------------------------------------------------------------------------

/// 一条在途命令。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inflight {
    pub fence_id: u64,
    pub cmd: CtrlCommand,
    /// 提交时刻（逻辑 tick µs）
    pub submit_tick: u64,
    /// 提交的 wire 长度
    pub wire_len: usize,
    pub head: u16,
    /// 设备是否已应答（收割前标记——同一 fence 的第二次应答就是串扰）
    pub answered: bool,
}

/// 超时产出：设备可疑记录（联动 VE-F0009 设备丢失状态机的输入件）。
///
/// F0009 的丢失检测消费本记录：连续可疑升级为丢失演练、
/// 恢复失败走软渲回退——本结构只负责如实上抛，不做处置决策。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceSuspicion {
    pub fence_id: u64,
    pub cmd_label: String,
    pub cmd_code: u32,
    pub submit_tick: u64,
    pub detected_tick: u64,
    pub age_us: u64,
    /// 归因提示（人话）
    pub note: String,
}

impl DeviceSuspicion {
    /// 联动 F0009 的固定注记——丢失状态机按此识别来源。
    pub fn links_f0009(&self) -> bool {
        self.note.contains("F0009")
    }
}

/// 提交/配对错误（三要素）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueError {
    pub code: &'static str,
    pub what: String,
    pub why: String,
    pub next: String,
}

// ---------------------------------------------------------------------------
// 三、控制队列引擎
// ---------------------------------------------------------------------------

/// 控制队列引擎：提交 → avail → 设备 → used → 收割配对。
pub struct ControlQueueEngine {
    pub table: DescTable,
    pub avail: AvailRing,
    pub used: UsedRing,
    inflight: Vec<Inflight>,
    next_fence: u64,
    now: u64,
    pub suspicions: Vec<DeviceSuspicion>,
    /// 串扰留痕（未知 fence 的应答记录）
    pub crosstalk: Vec<(u64, String)>,
    pub harvested: u64,
    pub submitted: u64,
}

impl ControlQueueEngine {
    pub fn new(desc_capacity: u16) -> ControlQueueEngine {
        ControlQueueEngine {
            table: DescTable::new(desc_capacity),
            avail: AvailRing::default(),
            used: UsedRing::default(),
            inflight: Vec::new(),
            next_fence: 1,
            now: 0,
            suspicions: Vec::new(),
            crosstalk: Vec::new(),
            harvested: 0,
            submitted: 0,
        }
    }

    pub fn now(&self) -> u64 {
        self.now
    }

    /// 推进逻辑时钟（µs）。
    pub fn advance(&mut self, us: u64) {
        self.now += us;
    }

    pub fn inflight(&self) -> &[Inflight] {
        &self.inflight
    }

    /// 提交一条命令：编码 → 组链 → 登记 avail → 记在途。
    ///
    /// fence_id 由引擎单调分配（token↔响应配对的唯一凭据）。
    pub fn submit(&mut self, cmd: &CtrlCommand) -> Result<u64, QueueError> {
        let fence = self.next_fence;
        let wire = cmd.encode(fence);
        let wire_len = wire.len();
        let head = self
            .table
            .push_chain(wire_len)
            .map_err(|e| QueueError {
                code: e.code,
                what: e.what,
                why: e.why,
                next: e.next,
            })?;
        self.avail.ring.push(head);
        self.avail.idx = self.avail.idx.wrapping_add(1);
        self.inflight.push(Inflight {
            fence_id: fence,
            cmd: cmd.clone(),
            submit_tick: self.now,
            wire_len,
            head,
            answered: false,
        });
        self.next_fence += 1;
        self.submitted += 1;
        Ok(fence)
    }

    /// 设备侧完成一条命令（写 used 环）。
    ///
    /// 未知 fence = 串扰；已应答过的 fence 再应答 = 重复串扰——
    /// 两种都显性拒绝并留痕（判据"在途配对零串扰"——串扰不是吞掉，
    /// 是被看见）。
    pub fn device_complete(
        &mut self,
        fence_id: u64,
        resp_wire_len: usize,
    ) -> Result<(), QueueError> {
        match self.inflight.iter_mut().find(|i| i.fence_id == fence_id) {
            Some(i) => {
                if i.answered {
                    self.crosstalk.push((
                        fence_id,
                        format!("tick {} 收到 fence {} 的重复应答", self.now, fence_id),
                    ));
                    return Err(QueueError {
                        code: "E_CROSSTALK",
                        what: format!("设备对 fence {} 重复应答", fence_id),
                        why: "一条命令-响应对只应答一次；第二次对不上在途配对"
                            .to_string(),
                        next: "丢弃该应答并按设备可疑处置（联动 F0009）".to_string(),
                    });
                }
                i.answered = true;
                let head = i.head;
                self.used.ring.push(UsedElem {
                    id: head as u32,
                    len: resp_wire_len as u32,
                });
                self.used.idx = self.used.idx.wrapping_add(1);
                Ok(())
            }
            None => {
                self.crosstalk.push((
                    fence_id,
                    format!("tick {} 收到未知 fence {} 的应答", self.now, fence_id),
                ));
                Err(QueueError {
                    code: "E_CROSSTALK",
                    what: format!(
                        "设备应答携带未知 fence {}（在途 {} 条）",
                        fence_id,
                        self.inflight.len()
                    ),
                    why: "fence 是命令-响应对的唯一凭据，对不上的应答属于串扰，\
错误路由会把别人的响应当自己的".to_string(),
                    next: "丢弃该应答并按设备可疑处置（联动 F0009）；\
检查设备侧是否重复应答或复位".to_string(),
                })
            }
        }
    }

    /// 收割 used 环：把完成条目与在途命令配对取出。
    ///
    /// 返回 (fence_id, 命令, 响应 wire 长度)。顺序 = 设备完成顺序（FIFO）。
    pub fn harvest(&mut self) -> Vec<(u64, CtrlCommand, usize)> {
        let mut out: Vec<(u64, CtrlCommand, usize)> = Vec::new();
        let items: Vec<UsedElem> = self.used.ring.drain(..).collect();
        for u in items {
            if let Some(pos) = self
                .inflight
                .iter()
                .position(|i| i.head as u32 == u.id)
            {
                let f = self.inflight.remove(pos);
                out.push((f.fence_id, f.cmd, u.len as usize));
                self.table.free_chain(f.head);
                self.harvested += 1;
            }
        }
        out
    }

    /// 超时巡检：把超龄在途命令转为设备可疑记录（联动 F0009）。
    ///
    /// 返回本次超时的条数。超时的命令从在途摘除——挂着不管只会
    /// 让在途表膨胀，摘除是显性的（留痕在 suspicions）。
    pub fn poll(&mut self) -> usize {
        let mut n = 0;
        let mut keep: Vec<Inflight> = Vec::new();
        for f in self.inflight.drain(..) {
            let age = self.now.saturating_sub(f.submit_tick);
            if age > CMD_TIMEOUT_US {
                self.suspicions.push(DeviceSuspicion {
                    fence_id: f.fence_id,
                    cmd_label: f.cmd.label().to_string(),
                    cmd_code: f.cmd.code(),
                    submit_tick: f.submit_tick,
                    detected_tick: self.now,
                    age_us: age,
                    note: format!(
                        "命令 {}（fence {}）超时 {}µs > 2s 默认——设备可疑，\
联动 F0009 设备丢失状态机（连续可疑升级丢失演练）",
                        f.cmd.label(),
                        f.fence_id,
                        age
                    ),
                });
                self.table.free_chain(f.head);
                n += 1;
            } else {
                keep.push(f);
            }
        }
        self.inflight = keep;
        n
    }

    /// 读屏可达状态摘要（规格：队列状态读屏可达）。
    pub fn a11y_summary(&self) -> String {
        format!(
            "virtio 控制队列：在途 {} 条、已提交 {}、已收割 {}、\
超时可疑 {} 条、串扰拦截 {} 次",
            self.inflight.len(),
            self.submitted,
            self.harvested,
            self.suspicions.len(),
            self.crosstalk.len()
        )
    }
}
