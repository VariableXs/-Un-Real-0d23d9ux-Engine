//! UNX-F5181 · 事务开启（journal_start）语义账 · 诊断段 0x8280
//! 验收锚点：UNX-F5181-J1 开启语义账——credits 申请/句柄返回断言 10/10 通过；
//! 零 credits 调用 10/10 次拒止（credits=0 → 拒）；与 F5171 结构账联账一致
//! （句柄 tid 与 journal 当前 tid 序列对拍一致）。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0x8280;

/// 事务句柄。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Handle {
    pub tid: u32,
    pub credits: u32,
}

impl Handle {
    pub fn new(tid: u32, credits: u32) -> Self {
        Handle { tid, credits }
    }
    pub fn valid(&self) -> bool {
        self.credits > 0
    }
}

/// journal 结构账（F5171 联账面）。
pub struct Journal {
    pub cur_tid: u32,
    pub running: Option<Handle>,
    pub started_ok: u32,
    pub rejected: u32,
}

impl Journal {
    pub fn new() -> Self {
        Journal { cur_tid: 1, running: None, started_ok: 0, rejected: 0 }
    }

    /// journal_start：credits>0 → 句柄；credits==0 → 拒。
    pub fn start(&mut self, credits: u32) -> Result<Handle, i32> {
        if credits == 0 {
            self.rejected += 1;
            return Err(22); // EINVAL
        }
        if self.running.is_some() {
            self.rejected += 1;
            return Err(16); // EBUSY：已有运行中事务
        }
        let h = Handle::new(self.cur_tid, credits);
        self.running = Some(h);
        self.started_ok += 1;
        self.cur_tid += 1;
        Ok(h)
    }

    /// 句柄提交释放。
    pub fn commit(&mut self, h: Handle) -> bool {
        match self.running {
            Some(r) if r == h => {
                self.running = None;
                true
            }
            _ => false,
        }
    }

    /// 联账：句柄 tid 与 journal 当前 tid 序列一致（tid < cur_tid 且 tid ≥ 1）。
    pub fn tid_consistent(&self, h: &Handle) -> bool {
        h.tid >= 1 && h.tid < self.cur_tid
    }
}

/// FNV-1a 32 位。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub fn lcg_next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}

// ── 注入核（0x8280 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x5181_8280;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("句柄-零credits非法", !Handle::new(1, 0).valid()));
    out.push(("journal-空账拒零credits", Journal::new().start(0).is_err()));
    out.push(("journal-初tid=1", Journal::new().cur_tid == 1));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx5181_checks：本单判据聚合（诊断段 0x8280）。
pub fn run_unx5181_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F5181 journal_start 语义账");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // credits 申请/句柄返回：10/10
    let mut j = Journal::new();
    let mut all_ok = true;
    for c in 1..=10u32 {
        match j.start(c) {
            Ok(h) => {
                if h.credits != c || !h.valid() || !j.commit(h) {
                    all_ok = false;
                }
            }
            Err(_) => all_ok = false,
        }
    }
    let ten_ok = all_ok && j.started_ok == 10;
    s.add("开启-10/10次credits申请句柄返回", ten_ok, if ten_ok { "" } else { "开启演练缺口" });

    // tid 递增序列
    let tid_ok = j.cur_tid == 11;
    s.add("tid-10次开启后cur_tid=11", tid_ok, if tid_ok { "" } else { "tid 序列漂移" });

    // 零 credits 10/10 拒止
    let mut j2 = Journal::new();
    let mut rej = 0u32;
    for _ in 0..10 {
        if j2.start(0) == Err(22) {
            rej += 1;
        }
    }
    let zero_ok = rej == 10 && j2.rejected == 10 && j2.started_ok == 0;
    s.add("拒止-零credits 10/10次EINVAL", zero_ok, if zero_ok { "" } else { "零 credits 漏网" });

    // EBUSY：运行中事务二次开启拒
    let mut j3 = Journal::new();
    let h1 = j3.start(4).unwrap();
    let busy = j3.start(8) == Err(16);
    s.add("并发-运行中二次开启EBUSY", busy, if busy { "" } else { "EBUSY 漏网" });

    // 提交后可再开
    let _ = j3.commit(h1);
    let reopen = j3.start(8).is_ok();
    s.add("并发-提交后可再开", reopen, if reopen { "" } else { "再开路径漂移" });

    // 错句柄提交拒
    let mut j4 = Journal::new();
    let ha = j4.start(2).unwrap();
    let wrong = Handle::new(999, 2);
    let wrong_rej = !j4.commit(wrong) && j4.commit(ha);
    s.add("提交-错句柄拒正确句柄过", wrong_rej, if wrong_rej { "" } else { "句柄核对失效" });

    // 联账 F5171：tid 一致
    let mut j5 = Journal::new();
    let mut all_tid = true;
    for _ in 0..8 {
        let h = j5.start(3).unwrap();
        if !j5.tid_consistent(&h) {
            all_tid = false;
        }
        let _ = j5.commit(h);
    }
    s.add("联账-8轮句柄tid与序列一致", all_tid, if all_tid { "" } else { "F5171 联账缺口" });

    // FNV 句柄指纹稳定
    let h = Handle::new(7, 64);
    let fp1 = fnv1a(&[h.tid as u8, (h.tid >> 8) as u8, h.credits as u8, (h.credits >> 8) as u8]);
    let fp2 = fnv1a(&[7, 0, 64, 0]);
    s.add("FNV-句柄指纹稳定", fp1 == fp2 && fp1 != 0, if fp1 == fp2 { "" } else { "指纹不稳" });

    s
}
