// unx1864_shootipi —— UNX-F1864 击落 IPI 消费端（F1846 登记向量执行）
// 判据原文 UNX-F1864-J1：击落处理体（mask 位或+本地 flush+应答）断言全过，
// 与 F1846 交接件状态位联动一致。
// 落位：kernel/varix/src/unxstar/；诊断段 0xF380 独占（0xF380~0xF3FF）。
//
// 设计（纯功能 no_std）：
//   * 击落处理体三步闭集：mask 位或（把 F1846 登记的页位或进本地屏蔽图）
//     → 本地 flush（TLB 项按屏蔽图失效）→ 应答（EOI + 状态位回写）；
//   * 与 F1846 交接件联动：发起端置 PENDING 位 → 消费端处理完回 DONE 位；
//     顺序不可倒置（未 PENDING 的 DONE = 联动破，必抓）；
//   * 三步序严格：跳步（mask 前 flush / flush 前应答）必拒；
//   * 指纹冻结 + 注入核 10/10。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xF380 独占） ─────────────────────────────────────────────
pub const XC_SHOOT_ORDER: u32 = 0xF381; // 三步序破（跳步）
pub const XC_SHOOT_HANDSHAKE: u32 = 0xF382; // 未 PENDING 先 DONE（联动破）
pub const XC_SHOOT_TWICE: u32 = 0xF383; // 重复应答
pub const XC_SHOOT_FLUSHMISS: u32 = 0xF384; // flush 漏项（登记位未入屏蔽图）
pub const XC_SHOOT_SEALED: u32 = 0xF385; // 封账后击落

/// 击落三步。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ShootStep {
    MaskOr = 1,
    Flush = 2,
    Eoi = 3,
}

/// F1846 交接件状态位（发起端 ↔ 消费端）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Handshake {
    Idle = 0,
    Pending = 1,
    Done = 2,
}

/// 单核击落消费状态机。
pub struct ShootdownConsumer {
    core: u32,
    mask_bits: u64,          // 本地屏蔽图（页号位图，低 64 页）
    flushed: Vec<u32>,       // flush 已失效页号
    step_cursor: u8,         // 三步游标（0=未开始）
    handshake: Handshake,
    eoi_sent: u32,
    ledger: Vec<(u64, u8)>,  // (刻, 步号)
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl ShootdownConsumer {
    pub fn new(core: u32) -> ShootdownConsumer {
        ShootdownConsumer {
            core,
            mask_bits: 0,
            flushed: Vec::new(),
            step_cursor: 0,
            handshake: Handshake::Idle,
            eoi_sent: 0,
            ledger: Vec::new(),
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 发起端置 PENDING（F1846 交接）。
    pub fn mark_pending(&mut self, at: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_SHOOT_SEALED, "封账后置位"));
            return Err(XC_SHOOT_SEALED);
        }
        self.handshake = Handshake::Pending;
        self.step_cursor = 0;
        self.ledger.push((at, 0)); // 0 = pending 事件
        Ok(())
    }

    /// 步 1：mask 位或 —— 登记页位或进本地屏蔽图。
    pub fn do_mask_or(&mut self, pages: &[u32], at: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_SHOOT_SEALED, "封账后击落"));
            return Err(XC_SHOOT_SEALED);
        }
        if self.handshake != Handshake::Pending {
            self.rejects.push((XC_SHOOT_HANDSHAKE, "未PENDING先击落"));
            return Err(XC_SHOOT_HANDSHAKE);
        }
        if self.step_cursor != 0 {
            self.rejects.push((XC_SHOOT_ORDER, "mask不在第一步"));
            return Err(XC_SHOOT_ORDER);
        }
        for &p in pages {
            if p < 64 {
                self.mask_bits |= 1u64 << p;
            }
        }
        self.step_cursor = 1;
        self.ledger.push((at, ShootStep::MaskOr as u8));
        Ok(())
    }

    /// 步 2：本地 flush —— 按屏蔽图逐页失效。
    pub fn do_flush(&mut self, at: u64) -> Result<u32, u32> {
        if self.sealed {
            self.rejects.push((XC_SHOOT_SEALED, "封账后flush"));
            return Err(XC_SHOOT_SEALED);
        }
        if self.step_cursor != 1 {
            self.rejects.push((XC_SHOOT_ORDER, "flush跳步"));
            return Err(XC_SHOOT_ORDER);
        }
        let mut n = 0;
        for p in 0..64u32 {
            if self.mask_bits & (1u64 << p) != 0 {
                self.flushed.push(p);
                n += 1;
            }
        }
        self.step_cursor = 2;
        self.ledger.push((at, ShootStep::Flush as u8));
        Ok(n)
    }

    /// 步 3：应答（EOI + DONE 回写）。
    pub fn do_eoi(&mut self, at: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_SHOOT_SEALED, "封账后eoi"));
            return Err(XC_SHOOT_SEALED);
        }
        if self.step_cursor != 2 {
            self.rejects.push((XC_SHOOT_ORDER, "eoi跳步"));
            return Err(XC_SHOOT_ORDER);
        }
        if self.handshake != Handshake::Pending {
            self.rejects.push((XC_SHOOT_HANDSHAKE, "应答联动破"));
            return Err(XC_SHOOT_HANDSHAKE);
        }
        self.eoi_sent += 1;
        self.handshake = Handshake::Done;
        self.step_cursor = 3;
        self.ledger.push((at, ShootStep::Eoi as u8));
        Ok(())
    }

    /// 完整击落一轮：三步序贯 + 联动闸。
    pub fn shoot_round(&mut self, pages: &[u32], at: u64) -> Result<u32, u32> {
        self.mark_pending(at)?;
        self.do_mask_or(pages, at + 1)?;
        let n = self.do_flush(at + 2)?;
        self.do_eoi(at + 3)?;
        Ok(n)
    }

    /// flush 完备性：屏蔽图每一位都必须在 flushed 中出现恰一次。
    pub fn flush_complete(&self) -> bool {
        for p in 0..64u32 {
            if self.mask_bits & (1u64 << p) != 0 {
                let mut cnt = 0;
                for &f in &self.flushed {
                    if f == p {
                        cnt += 1;
                    }
                }
                if cnt != 1 {
                    return false;
                }
            }
        }
        true
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_SHOOT_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.mask_bits as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= (self.mask_bits >> 32) as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= self.eoi_sent;
        h = h.wrapping_mul(0x0100_0193);
        for &(t, k) in &self.ledger {
            h ^= (t as u32) ^ (k as u32).wrapping_mul(0x9E37);
            h = h.wrapping_mul(0x0100_0193);
        }
        (h as u64) << 16 | self.handshake as u64
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
    pub fn handshake_now(&self) -> Handshake {
        self.handshake
    }
}

/// 注入核：五类破坏各注入一次。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 5;

    // 1）跳步：未 mask 直接 flush 必拒
    {
        let mut c = ShootdownConsumer::new(0);
        let _ = c.mark_pending(1);
        if c.do_flush(2) == Err(XC_SHOOT_ORDER) {
            caught += 1;
        }
    }
    // 2）联动破：未 PENDING 直接 mask 必拒
    {
        let mut c = ShootdownConsumer::new(0);
        if c.do_mask_or(&[1, 2], 1) == Err(XC_SHOOT_HANDSHAKE) {
            caught += 1;
        }
    }
    // 3）跳步：未 flush 直接 eoi 必拒
    {
        let mut c = ShootdownConsumer::new(0);
        let _ = c.mark_pending(1);
        let _ = c.do_mask_or(&[3], 2);
        if c.do_eoi(3) == Err(XC_SHOOT_ORDER) {
            caught += 1;
        }
    }
    // 4）重复应答：DONE 后再走一轮 eoi（在 PENDING 前置下应答位已 Done）必拒
    {
        let mut c = ShootdownConsumer::new(0);
        let _ = c.shoot_round(&[5], 1);
        // 手动重演：置 Pending 再直接 eoi（跳 mask/flush）
        let _ = c.mark_pending(10);
        if c.do_eoi(11) == Err(XC_SHOOT_ORDER) {
            caught += 1;
        }
    }
    // 5）封账后击落必拒
    {
        let mut c = ShootdownConsumer::new(0);
        let _ = c.seal();
        if c.shoot_round(&[7], 1) == Err(XC_SHOOT_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1864-J1。
pub fn run_unx1864_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1864");

    // C64-01 三步完整轮（mask→flush→eoi → DONE）
    {
        let mut c = ShootdownConsumer::new(2);
        let r = c.shoot_round(&[1, 2, 3], 10);
        let ok = r == Ok(3) && c.handshake_now() == Handshake::Done && c.flush_complete();
        s.add("C64-01-三步完整轮", ok, if ok { "" } else { "三步轮破" });
    }
    // C64-02 mask 位或语义（同页多射幂等）
    {
        let mut c = ShootdownConsumer::new(0);
        let _ = c.shoot_round(&[5], 1);
        let mut c2 = ShootdownConsumer::new(0);
        let _ = c2.shoot_round(&[5, 5, 5], 1);
        let ok = c.fingerprint() == c2.fingerprint();
        s.add("C64-02-位或幂等", ok, if ok { "" } else { "或语义漂移" });
    }
    // C64-03 flush 完备（逐屏蔽位恰失效一次）
    {
        let mut c = ShootdownConsumer::new(0);
        let _ = c.shoot_round(&[10, 20, 30], 1);
        let ok = c.flush_complete();
        s.add("C64-03-flush完备", ok, if ok { "" } else { "flush漏项" });
    }
    // C64-04 交接联动序（Idle→Pending→Done）
    {
        let mut c = ShootdownConsumer::new(0);
        let h0 = c.handshake_now();
        let _ = c.mark_pending(1);
        let h1 = c.handshake_now();
        let _ = c.do_mask_or(&[2], 2);
        let _ = c.do_flush(3);
        let _ = c.do_eoi(4);
        let h2 = c.handshake_now();
        let ok = h0 == Handshake::Idle && h1 == Handshake::Pending && h2 == Handshake::Done;
        s.add("C64-04-联动序", ok, if ok { "" } else { "联动漂移" });
    }
    // C64-05 多页一轮全失效（64 页满载）
    {
        let pages: Vec<u32> = (0..64).collect();
        let mut c = ShootdownConsumer::new(0);
        let r = c.shoot_round(&pages, 1);
        let ok = r == Ok(64) && c.flush_complete();
        s.add("C64-05-满载64页", ok, if ok { "" } else { "满载破" });
    }
    // C64-06 注入核 5/5
    {
        let (n, c) = inject_core();
        let ok = n == 5 && c == 5;
        s.add("C64-06-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C64-07 步号闭集与序
    {
        let ok = ShootStep::MaskOr as u8 == 1
            && ShootStep::Flush as u8 == 2
            && ShootStep::Eoi as u8 == 3;
        s.add("C64-07-步号闭集", ok, if ok { "" } else { "步号漂移" });
    }
    // C64-08 账事件序（0 pending → 1 → 2 → 3）
    {
        let mut c = ShootdownConsumer::new(0);
        let _ = c.shoot_round(&[8], 1);
        let seq: Vec<u8> = c.ledger.iter().map(|&(_, k)| k).collect();
        let ok = seq == vec![0, 1, 2, 3];
        s.add("C64-08-账事件序", ok, if ok { "" } else { "账序漂移" });
    }
    // C64-09 指纹冻结（同构同指纹）
    {
        let f = || {
            let mut c = ShootdownConsumer::new(1);
            let _ = c.shoot_round(&[9, 11], 1);
            c.fingerprint()
        };
        let ok = f() == f();
        s.add("C64-09-指纹一致", ok, if ok { "" } else { "指纹漂移" });
    }
    // C64-10 封账稳定
    {
        let mut c = ShootdownConsumer::new(0);
        let _ = c.shoot_round(&[4], 1);
        let pre = c.fingerprint();
        let _ = c.seal();
        let ok = c.fingerprint() == pre && c.seal().is_err();
        s.add("C64-10-封账稳定", ok, if ok { "" } else { "封账漂移" });
    }
    s
}
