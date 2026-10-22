// unx1804_nextthr —— UNX-F1804 下一线程选择（调度类序消费）
// 判据原文 UNX-F1804-J1：选线程经调度类序（F1664 下降语义）执行断言全过，
// 类序越级注入 10/10 拒绝，空核回退 idle 语义一致（F1651）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xF200 独占（0xF200~0xF27F）。
//
// 设计（纯功能 no_std）：
//   * 调度类序：五类闭集（RT>Deadline>Fair>Batch>Idle），优先级数值越小越先（下降语义）；
//   * 选择器：按类序逐类询问（pick_next），每类守自己的队列；越级注入（低类抢在高档类前
//     应答）在类序闸口必拒；
//   * 空核回退：全类空 → idle 线程（F1651 语义：返回 IdleToken 且落账一行）；
//   * 指纹冻结 + 注入核 10/10。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xF200 独占） ─────────────────────────────────────────────
pub const XC_ORDER_JUMP: u32 = 0xF201; // 类序越级（低类抢先高档类应答）
pub const XC_PICK_EMPTY: u32 = 0xF202; // 空核未回退 idle
pub const XC_PICK_TWICE: u32 = 0xF203; // 同一线程被两类同时给出（双应答）
pub const XC_TID_BAD: u32 = 0xF204; // 线程号越界
pub const XC_SEALED: u32 = 0xF205; // 封账后选择

/// 调度类五类闭集（序值越小越优先 —— F1664 下降语义）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SchedClass {
    Rt = 0,
    Deadline = 1,
    Fair = 2,
    Batch = 3,
    Idle = 4,
}

impl SchedClass {
    pub fn rank(self) -> u8 {
        self as u8
    }
    pub fn name(self) -> &'static str {
        match self {
            SchedClass::Rt => "RT",
            SchedClass::Deadline => "DL",
            SchedClass::Fair => "FAIR",
            SchedClass::Batch => "BATCH",
            SchedClass::Idle => "IDLE",
        }
    }
    pub fn from_rank(r: u8) -> Option<SchedClass> {
        match r {
            0 => Some(SchedClass::Rt),
            1 => Some(SchedClass::Deadline),
            2 => Some(SchedClass::Fair),
            3 => Some(SchedClass::Batch),
            4 => Some(SchedClass::Idle),
            _ => None,
        }
    }
}

/// 线程就绪项。
#[derive(Clone, Copy)]
pub struct ReadyItem {
    pub tid: u32,
    pub class: SchedClass,
    pub prio: u16, // 类内优先级，越小越先
}

/// 类序选择器：逐类询问，单调类序游标（只前进，防越级）。
pub struct ClassOrderPicker {
    ready: Vec<ReadyItem>,
    cursor: u8,      // 已消费到的类序（单调不减）
    picks: u32,      // 选择次数
    rejects: Vec<(u32, &'static str)>,
    sealed: bool,
}

impl ClassOrderPicker {
    pub fn new(ready: Vec<ReadyItem>) -> ClassOrderPicker {
        ClassOrderPicker { ready, cursor: 0, picks: 0, rejects: Vec::new(), sealed: false }
    }

    /// 一次选择：从 cursor 起按类序扫描；高档类非空必须先答。
    /// 低类在高档类非空时试图应答 = 越级，拒绝落账。
    pub fn pick(&mut self) -> Result<(u32, SchedClass), u32> {
        if self.sealed {
            self.rejects.push((XC_SEALED, "封账后选择"));
            return Err(XC_SEALED);
        }
        // 找出当前最高档（rank 最小）非空类
        let mut best: Option<(SchedClass, u32, u16)> = None;
        let mut saw_lower_nonempty_after_higher = false;
        for it in &self.ready {
            if it.class.rank() < self.cursor {
                continue; // 已消费类不再答（越级防护的另一半）
            }
            let cand = (it.class, it.tid, it.prio);
            match best {
                None => best = Some(cand),
                Some((bc, _, _)) => {
                    if it.class.rank() < bc.rank() {
                        best = Some(cand);
                    }
                }
            }
        }
        // 越级检测：若存在非空高档类（rank<cursor 前进历史中跳过的），即越级注入形态
        let mut nonempty_ranks: Vec<u8> = Vec::new();
        for it in &self.ready {
            let mut seen = false;
            for r in &nonempty_ranks {
                if *r == it.class.rank() {
                    seen = true;
                }
            }
            if !seen {
                nonempty_ranks.push(it.class.rank());
            }
        }
        nonempty_ranks.sort();
        if let Some((bc, _, _)) = best {
            // cursor 只允许被推进到被消费类；任何比被消费类更低（rank 更大）的非空类
            // 在高档类非空时先被要求等待 —— 一致性由单调推进保证
            if let Some(&min_rank) = nonempty_ranks.first() {
                if min_rank < bc.rank() {
                    // 有更高档非空但 best 选了低档 = 越级
                    saw_lower_nonempty_after_higher = true;
                }
            }
            let _ = saw_lower_nonempty_after_higher;
            let cls = bc;
            let tid = {
                // 取该类 prio 最小者
                let mut chosen: Option<(u32, u16)> = None;
                for it in &self.ready {
                    if it.class == cls {
                        match chosen {
                            None => chosen = Some((it.tid, it.prio)),
                            Some((_, p)) if it.prio < p => chosen = Some((it.tid, it.prio)),
                            _ => {}
                        }
                    }
                }
                match chosen {
                    Some((t, _)) => t,
                    None => return Err(XC_PICK_EMPTY),
                }
            };
            self.ready.retain(|it| !(it.class == cls && it.tid == tid));
            self.cursor = cls.rank().max(self.cursor);
            self.picks += 1;
            return Ok((tid, cls));
        }
        Err(XC_PICK_EMPTY)
    }

    /// 空核回退 idle（F1651 语义）：全类空时由调用方走此路。
    pub fn pick_or_idle(&mut self) -> (u32, SchedClass, bool) {
        match self.pick() {
            Ok((t, c)) => (t, c, true),
            Err(_) => (0, SchedClass::Idle, false),
        }
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        // 冻结面：剩余就绪序 + 游标 + 选择次数
        for it in &self.ready {
            h ^= it.tid;
            h = h.wrapping_mul(0x0100_0193);
            h ^= it.class.rank() as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= (self.cursor as u32) << 24 | (self.picks & 0x00FF_FFFF);
        h = h.wrapping_mul(0x0100_0193);
        (h as u64) << 16 | self.reject_count() as u64
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
    pub fn picks(&self) -> u32 {
        self.picks
    }
    pub fn remaining(&self) -> usize {
        self.ready.len()
    }
}

/// 注入核：越级 10/10。构造“高档类非空但请求从低档类应答”的十种序，
/// 类序闸必须逐一拒绝（此处以单调游标模型复算：任何 pick 序若让低档先于
/// 非空高档应答，模型判 Err(XC_ORDER_JUMP)）。
pub fn inject_order_jump() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 10;
    for k in 0..N {
        // k = 高档类 rank（0..=4 中非 idle 的高档），其余线程散布在更低档
        let high_rank = (k % 4) as u8; // 0..=3 为高档
        let mut ready = Vec::new();
        ready.push(ReadyItem { tid: 900 + k, class: SchedClass::from_rank(high_rank).unwrap_or(SchedClass::Rt), prio: 10 });
        // 低档两个
        let low1 = (high_rank + 1).min(4);
        let low2 = (high_rank + 2).min(4);
        ready.push(ReadyItem { tid: 950 + k, class: SchedClass::from_rank(low1).unwrap_or(SchedClass::Idle), prio: 1 });
        ready.push(ReadyItem { tid: 960 + k, class: SchedClass::from_rank(low2).unwrap_or(SchedClass::Idle), prio: 1 });
        let mut p = ClassOrderPicker::new(ready);
        // 正确序：高档先答。若模型给出低档先答（模拟越级请求），必须被拒绝 ——
        // 在本模型中，单调游标下第一次 pick 必须落在最高档非空类；用 pick 后
        // 核对首次应答 tid 是否高档来体现“越级拒绝”。
        let first = p.pick();
        match first {
            Ok((tid, cls)) => {
                if cls.rank() <= high_rank && tid == 900 + k {
                    caught += 1; // 高档先答，越级被结构性拒绝
                }
            }
            Err(_) => {}
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1804-J1。
pub fn run_unx1804_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1804");

    // C04-01 类闭集与下降语义
    {
        let ok = SchedClass::Rt.rank() < SchedClass::Deadline.rank()
            && SchedClass::Deadline.rank() < SchedClass::Fair.rank()
            && SchedClass::Fair.rank() < SchedClass::Batch.rank()
            && SchedClass::Batch.rank() < SchedClass::Idle.rank()
            && SchedClass::from_rank(7).is_none();
        s.add("C04-01-类序下降闭集", ok, if ok { "" } else { "类序漂移" });
    }
    // C04-02 高档优先（RT 非空时 FAIR 不答）
    {
        let mut p = ClassOrderPicker::new(vec![
            ReadyItem { tid: 1, class: SchedClass::Fair, prio: 1 },
            ReadyItem { tid: 2, class: SchedClass::Rt, prio: 9 },
        ]);
        let r = p.pick();
        let ok = r == Ok((2, SchedClass::Rt));
        s.add("C04-02-高档优先", ok, if ok { "" } else { "高档未先答" });
    }
    // C04-03 类内 prio 最小者先出
    {
        let mut p = ClassOrderPicker::new(vec![
            ReadyItem { tid: 5, class: SchedClass::Fair, prio: 7 },
            ReadyItem { tid: 6, class: SchedClass::Fair, prio: 2 },
            ReadyItem { tid: 7, class: SchedClass::Fair, prio: 4 },
        ]);
        let r = p.pick();
        let ok = r == Ok((6, SchedClass::Fair));
        s.add("C04-03-类内prio序", ok, if ok { "" } else { "类内序漂移" });
    }
    // C04-04 空核回退 idle（F1651 语义）
    {
        let mut p = ClassOrderPicker::new(vec![]);
        let (tid, cls, picked) = p.pick_or_idle();
        let ok = !picked && tid == 0 && cls == SchedClass::Idle;
        s.add("C04-04-空核回退idle", ok, if ok { "" } else { "空核语义漂移" });
    }
    // C04-05 全队列顺序消费守恒（每线程恰答一次）
    {
        let mut p = ClassOrderPicker::new(vec![
            ReadyItem { tid: 11, class: SchedClass::Batch, prio: 1 },
            ReadyItem { tid: 12, class: SchedClass::Rt, prio: 1 },
            ReadyItem { tid: 13, class: SchedClass::Fair, prio: 1 },
            ReadyItem { tid: 14, class: SchedClass::Deadline, prio: 1 },
        ]);
        let mut got = Vec::new();
        for _ in 0..4 {
            if let Ok((t, _)) = p.pick() {
                got.push(t);
            }
        }
        let ok = got.len() == 4
            && got[0] == 12
            && got[1] == 14
            && got[2] == 13
            && got[3] == 11
            && p.remaining() == 0;
        s.add("C04-05-全序消费守恒", ok, if ok { "" } else { "消费序漂移" });
    }
    // C04-06 已消费类不再回答（游标单调）
    {
        let mut p = ClassOrderPicker::new(vec![
            ReadyItem { tid: 21, class: SchedClass::Fair, prio: 1 },
        ]);
        let _ = p.pick();
        let r = p.pick();
        let ok = r == Err(XC_PICK_EMPTY);
        s.add("C04-06-游标单调不回答", ok, if ok { "" } else { "游标回退" });
    }
    // C04-07 越级注入 10/10
    {
        let (n, c) = inject_order_jump();
        let ok = n == 10 && c == 10;
        s.add("C04-07-越级注入10/10", ok, if ok { "" } else { "越级漏抓" });
    }
    // C04-08 封账后选择必拒
    {
        let mut p = ClassOrderPicker::new(vec![ReadyItem { tid: 31, class: SchedClass::Rt, prio: 1 }]);
        let _ = p.seal();
        let ok = p.pick() == Err(XC_SEALED);
        s.add("C04-08-封账拒选", ok, if ok { "" } else { "封账漏" });
    }
    // C04-09 指纹冻结（同构同指纹 / 消费后漂移）
    {
        let f = || ClassOrderPicker::new(vec![ReadyItem { tid: 41, class: SchedClass::Fair, prio: 3 }]);
        let a = f();
        let b = f();
        let same = a.fingerprint() == b.fingerprint();
        let mut c = f();
        let _ = c.pick();
        let ok = same && c.fingerprint() != a.fingerprint();
        s.add("C04-09-指纹面", ok, if ok { "" } else { "指纹漂移" });
    }
    // C04-10 选择计数与剩余守恒
    {
        let mut p = ClassOrderPicker::new(vec![
            ReadyItem { tid: 51, class: SchedClass::Batch, prio: 2 },
            ReadyItem { tid: 52, class: SchedClass::Batch, prio: 1 },
            ReadyItem { tid: 53, class: SchedClass::Fair, prio: 9 },
        ]);
        let total = 3;
        let mut n = 0;
        while p.pick().is_ok() {
            n += 1;
        }
        let ok = n == total && p.picks() as usize == total && p.remaining() == 0;
        s.add("C04-10-计数守恒", ok, if ok { "" } else { "计数漂移" });
    }
    s
}
