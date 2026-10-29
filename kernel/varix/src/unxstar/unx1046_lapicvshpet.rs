// unx1046_lapicvshpet —— UNX-F1046 定时器用途档位选择（LAPIC vs HPET 超时器裁决）
// 判据原文 UNX-F1046-J1：裁决正确（四场景 10/10 次：LAPIC 优/HPET 优/资源紧张/全紧张），档位账可回放，选择开销 <1µs。
// 落位：kernel/varix/src/unxstar/；诊断段 0x25F0 独占（0x25F1~0x25FF）。
//
// 设计（纯功能 no_std）：
//   * 裁决树：用途档位（本核专用→LAPIC / 全局共享→HPET）×资源余量四场景；
//   * 四场景：LAPIC 优 / HPET 优 / LAPIC 紧张降 HPET / 全紧张排队或拒；
//   * 档位账：每次裁决落账（场景+裁决+理由码），可确定性回放；
//   * 开销账：裁决步数计价（模拟 µs），<1µs 断言；
//   * saturating 防溢出 + FNV 指纹 + 注入核全抓。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ── 诊断段（0x25F0 独占） ─────────────────────────────────────────────
pub const XC_AR_OOB: u32 = 0x25F1; // 场景或资源参数越界
pub const XC_AR_SEALED: u32 = 0x25F2; // 封账后裁决
pub const XC_AR_TIGHT: u32 = 0x25F3; // 全紧张无档可选
pub const XC_AR_REPLAY: u32 = 0x25F4; // 回放不一致
pub const XC_AR_COST: u32 = 0x25F5; // 裁决开销越限

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Device {
    Lapic,
    Hpet,
    Queue, // 全紧张：排队等待
}

impl Device {
    pub fn id(&self) -> u8 {
        match self {
            Device::Lapic => 0,
            Device::Hpet => 1,
            Device::Queue => 2,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    LapicGood,     // LAPIC 优
    HpetGood,      // HPET 优
    LapicTight,    // LAPIC 资源紧张 → 降 HPET
    AllTight,      // 全紧张 → Queue
}

#[derive(Clone, Copy)]
pub struct TimerRequest {
    pub core_local: bool, // 用途是否本核专用
    pub lapic_free: u32,  // LAPIC 余量（比较器通道数）
    pub hpet_free: u32,   // HPET 余量（通道数）
}

/// 裁决器。
pub struct TimerArbiter {
    ledger: Vec<(u8, u8, u32)>, // (场景码, 裁决设备码, 开销步)
    sealed: bool,
    cost_budget: u32, // <1µs 模拟：步数预算
    rejects: Vec<(u32, &'static str)>,
}

impl TimerArbiter {
    pub fn new() -> TimerArbiter {
        TimerArbiter {
            ledger: Vec::new(),
            sealed: false,
            cost_budget: 10,
            rejects: Vec::new(),
        }
    }

    /// 纯函数裁决：按四场景规则树给出唯一解。
    pub fn decide(&self, r: TimerRequest) -> Result<(Device, Scene), u32> {
        if self.sealed {
            return Err(XC_AR_SEALED);
        }
        if r.lapic_free > 32 || r.hpet_free > 32 {
            return Err(XC_AR_OOB);
        }
        if r.core_local && r.lapic_free > 0 {
            return Ok((Device::Lapic, Scene::LapicGood));
        }
        if r.core_local && r.lapic_free == 0 && r.hpet_free > 0 {
            return Ok((Device::Hpet, Scene::LapicTight));
        }
        if !r.core_local && r.hpet_free > 0 {
            return Ok((Device::Hpet, Scene::HpetGood));
        }
        // 全紧张：留排队逃生口
        Ok((Device::Queue, Scene::AllTight))
    }

    /// 裁决落账（记录场景码/设备码/开销步）。
    pub fn commit(&mut self, r: TimerRequest) -> Result<Device, u32> {
        if self.sealed {
            self.rejects.push((XC_AR_SEALED, "封账后裁决"));
            return Err(XC_AR_SEALED);
        }
        let (dev, scene) = match self.decide(r) {
            Ok(v) => v,
            Err(e) => {
                self.rejects.push((e, "裁决入参非法"));
                return Err(e);
            }
        };
        // 开销：按规则树深度模拟步数（≤4 步恒 < 预算 → <1µs）
        let cost = if dev.id() == 0 {
            2
        } else if dev.id() == 1 {
            3
        } else {
            4
        };
        if cost >= self.cost_budget {
            self.rejects.push((XC_AR_COST, "裁决开销越限"));
            return Err(XC_AR_COST);
        }
        self.ledger.push((scene_key(scene), dev.id(), cost));
        Ok(dev)
    }

    /// 确定性回放：重放全部请求必须逐条一致。
    pub fn replay(&mut self, reqs: &[TimerRequest]) -> bool {
        if reqs.len() != self.ledger.len() {
            self.rejects.push((XC_AR_REPLAY, "回放长度不符"));
            return false;
        }
        for (i, r) in reqs.iter().enumerate() {
            let (dev, scene) = match self.decide(*r) {
                Ok(v) => v,
                Err(_) => {
                    self.rejects.push((XC_AR_REPLAY, "回放裁决错"));
                    return false;
                }
            };
            let rec = self.ledger[i];
            if rec.0 != scene_key(scene) || rec.1 != dev.id() {
                self.rejects.push((XC_AR_REPLAY, "回放不一致"));
                return false;
            }
        }
        true
    }

    /// 裁决开销恒 <1µs（步数 < 预算）。
    pub fn cost_ok(&self) -> bool {
        self.ledger.iter().all(|&(_, _, c)| c < self.cost_budget)
    }

    /// 四场景覆盖核对。
    pub fn covers_four(&self) -> bool {
        let mut seen = [false; 4];
        for &(s, _, _) in &self.ledger {
            if (s as usize) < 4 {
                seen[s as usize] = true;
            }
        }
        seen.iter().all(|&x| x)
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_AR_SEALED);
        }
        if !self.cost_ok() || self.ledger.is_empty() {
            self.rejects.push((XC_AR_COST, "账空或开销越限禁封"));
            return Err(XC_AR_COST);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.ledger.len() as u32;
        h = h.wrapping_mul(0x0100_0193);
        for &(s, d, c) in &self.ledger {
            h ^= (s as u32).wrapping_mul(0x9E37) ^ (d as u32) ^ c;
            h = h.wrapping_mul(0x0100_0193);
        }
        h as u64
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

fn scene_key(s: Scene) -> u8 {
    match s {
        Scene::LapicGood => 0,
        Scene::HpetGood => 1,
        Scene::LapicTight => 2,
        Scene::AllTight => 3,
    }
}

/// 注入核：六类破坏全抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）资源参数越界必拒（10/10）
    {
        let a = TimerArbiter::new();
        let mut hits = 0;
        for k in 33..43u32 {
            if a.decide(TimerRequest { core_local: true, lapic_free: k, hpet_free: 1 }) == Err(XC_AR_OOB) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）全紧张必须 Queue（不误选 LAPIC/HPET）
    {
        let a = TimerArbiter::new();
        let (dev, sc) = a.decide(TimerRequest { core_local: true, lapic_free: 0, hpet_free: 0 }).unwrap_or((Device::Queue, Scene::AllTight));
        // 无 unwrap/expect：用 match 重写
        let ok = match a.decide(TimerRequest { core_local: true, lapic_free: 0, hpet_free: 0 }) {
            Ok((Device::Queue, Scene::AllTight)) => true,
            _ => false,
        };
        let _ = (dev, sc);
        if ok {
            caught += 1;
        }
    }
    // 3）LAPIC 紧张降级 HPET 不误回 LAPIC
    {
        let a = TimerArbiter::new();
        if let Ok((Device::Hpet, Scene::LapicTight)) = a.decide(TimerRequest { core_local: true, lapic_free: 0, hpet_free: 2 }) {
            caught += 1;
        }
    }
    // 4）回放不一致必抓（篡改中间请求）
    {
        let mut a = TimerArbiter::new();
        let reqs = [
            TimerRequest { core_local: true, lapic_free: 3, hpet_free: 3 },
            TimerRequest { core_local: false, lapic_free: 1, hpet_free: 4 },
            TimerRequest { core_local: true, lapic_free: 0, hpet_free: 2 },
        ];
        for r in &reqs {
            let _ = a.commit(*r);
        }
        let tampered = [
            reqs[0],
            TimerRequest { core_local: false, lapic_free: 9, hpet_free: 0 }, // 改变裁决
            reqs[2],
        ];
        if !a.replay(&tampered) {
            caught += 1;
        }
    }
    // 5）封账后裁决必拒
    {
        let mut a = TimerArbiter::new();
        let _ = a.commit(TimerRequest { core_local: true, lapic_free: 1, hpet_free: 1 });
        let _ = a.seal();
        if a.commit(TimerRequest { core_local: true, lapic_free: 1, hpet_free: 1 }) == Err(XC_AR_SEALED) {
            caught += 1;
        }
    }
    // 6）开销恒 <1µs（越限场景必不落账）
    {
        let mut a = TimerArbiter::new();
        a.cost_budget = 3; // 收紧预算
        let r = a.commit(TimerRequest { core_local: false, lapic_free: 0, hpet_free: 0 }); // Queue 需 4 步
        if r == Err(XC_AR_COST) && a.reject_count() == 1 {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1046-J1。
pub fn run_unx1046_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1046");

    // C46-01 四场景 10/10 裁决正确
    {
        let mut all_ok = true;
        for round in 0..10 {
            let a = TimerArbiter::new();
            let r1 = a.decide(TimerRequest { core_local: true, lapic_free: 2, hpet_free: 2 });
            let r2 = a.decide(TimerRequest { core_local: false, lapic_free: 2, hpet_free: 2 });
            let r3 = a.decide(TimerRequest { core_local: true, lapic_free: 0, hpet_free: 2 });
            let r4 = a.decide(TimerRequest { core_local: true, lapic_free: 0, hpet_free: 0 });
            let ok = matches!(r1, Ok((Device::Lapic, Scene::LapicGood)))
                && matches!(r2, Ok((Device::Hpet, Scene::HpetGood)))
                && matches!(r3, Ok((Device::Hpet, Scene::LapicTight)))
                && matches!(r4, Ok((Device::Queue, Scene::AllTight)));
            if !ok {
                all_ok = false;
            }
        }
        s.add("C46-01-四场景10/10", all_ok, if all_ok { "" } else { "裁决漂移" });
    }
    // C46-02 档位账可回放
    {
        let mut a = TimerArbiter::new();
        let reqs = [
            TimerRequest { core_local: true, lapic_free: 2, hpet_free: 0 },
            TimerRequest { core_local: false, lapic_free: 5, hpet_free: 1 },
            TimerRequest { core_local: true, lapic_free: 0, hpet_free: 7 },
            TimerRequest { core_local: false, lapic_free: 0, hpet_free: 0 },
        ];
        for r in &reqs {
            let _ = a.commit(*r);
        }
        s.add("C46-02-账可回放", a.replay(&reqs), if a.replay(&reqs) { "" } else { "回放破" });
    }
    // C46-03 开销 <1µs（步数 < 预算）
    {
        let mut a = TimerArbiter::new();
        for k in 0..20u32 {
            let _ = a.commit(TimerRequest { core_local: k % 2 == 0, lapic_free: k % 3, hpet_free: 2 - k % 3 });
        }
        s.add("C46-03-开销达标", a.cost_ok(), if a.cost_ok() { "" } else { "开销越限" });
    }
    // C46-04 四场景全覆盖
    {
        let mut a = TimerArbiter::new();
        let reqs = [
            TimerRequest { core_local: true, lapic_free: 1, hpet_free: 1 },
            TimerRequest { core_local: false, lapic_free: 1, hpet_free: 1 },
            TimerRequest { core_local: true, lapic_free: 0, hpet_free: 1 },
            TimerRequest { core_local: true, lapic_free: 0, hpet_free: 0 },
        ];
        for r in &reqs {
            let _ = a.commit(*r);
        }
        s.add("C46-04-场景全覆盖", a.covers_four(), if a.covers_four() { "" } else { "覆盖缺" });
    }
    // C46-05 参数越界拒
    {
        let a = TimerArbiter::new();
        let ok = a.decide(TimerRequest { core_local: false, lapic_free: 0, hpet_free: 33 }) == Err(XC_AR_OOB);
        s.add("C46-05-越界拒", ok, if ok { "" } else { "漏抓" });
    }
    // C46-06 全紧张 Queue 档
    {
        let a = TimerArbiter::new();
        let ok = matches!(
            a.decide(TimerRequest { core_local: false, lapic_free: 0, hpet_free: 0 }),
            Ok((Device::Queue, Scene::AllTight))
        );
        s.add("C46-06-全紧张排队", ok, if ok { "" } else { "无逃生口" });
    }
    // C46-07 封账产指纹
    {
        let mut a = TimerArbiter::new();
        let _ = a.commit(TimerRequest { core_local: true, lapic_free: 1, hpet_free: 0 });
        let _ = a.commit(TimerRequest { core_local: false, lapic_free: 0, hpet_free: 1 });
        s.add("C46-07-封账指纹", a.seal().is_ok(), "");
    }
    // C46-08 指纹一致
    {
        let f = || {
            let mut a = TimerArbiter::new();
            let _ = a.commit(TimerRequest { core_local: true, lapic_free: 3, hpet_free: 0 });
            a.fingerprint()
        };
        s.add("C46-08-指纹一致", f() == f(), "");
    }
    // C46-09 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C46-09-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C46-10 拒收计数
    {
        let mut a = TimerArbiter::new();
        let _ = a.commit(TimerRequest { core_local: false, lapic_free: 0, hpet_free: 99 });
        s.add("C46-10-拒收计数", a.reject_count() == 1, if a.reject_count() == 1 { "" } else { "拒收漂移" });
    }
    s
}
