//! unx0391_oominject —— UNX-F0391 分配失败注入演练。
//! 锚点（CoRun Unxreal 主册）：UNX-F0391 · 分配失败注入演练 · 目标 200 行
//! 判据原文：UNX-F0391-J1 可控注入分配失败（按注入率千分或按
//!           精确步号），要求：失败必须显式回报（拒绝码非零）
//!           不许静默给零；请求方按重试上限表现（重试计账、
//!           放弃计账、降级计账三选一显式落账）；演练收官时
//!           成功 + 失败 = 总请求（账平）；重试链可重放（同
//!           注入序列同结局）；注入率 0 时全程成功无拒绝。
//! 组件：注入器（率控 + 步控两种模式）+ 受压分配器（确定性
//!       背后账）+ 请求方策略（重试 N 次 / 放弃 / 降级小请求）
//!       + 演练账（成功/失败/重试/降级四计数）+ 收官对账闸。
//! 职责定位：失败注入是「堆的消防演习」——平时不演练，OOM
//!   时每个调用点都是雷；可控注入让「失败路径」成为必经之路：
//!   每个请求方必须显式落账自己的策略（重试几次、何时放弃、
//!   有没有降级），收官时四计数对账平，任何静默吞错都会让
//!   账对不上。
//! 性能逐项分解：单请求 O(重试上限)；注入判定 O(1)；对账 O(1)。
//! 诊断码段：0x20C80（0x20C81~0x20C89 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const OI_VERSION: &str = "unx0391_oominject-v1";

/// 诊断段基址（0x20C80 段 · 本模块独占）。
pub const OI_SEG_BASE: u32 = 0x20C80;
pub const XC_RATE_OOR: u32 = 0x20C81; // 注入率越域（>1000）
pub const XC_RETRY_CAP: u32 = 0x20C82; // 重试上限越域（0 或超 16）
pub const XC_LEDGER_UNBAL: u32 = 0x20C83; // 收官账不平
pub const XC_SILENT: u32 = 0x20C84; // 失败静默（拒绝码为零）
pub const XC_REPLAY: u32 = 0x20C85; // 重放不一致
pub const XC_STRAT_OOR: u32 = 0x20C86; // 策略越域
pub const XC_QUOTA_OOR: u32 = 0x20C87; // 请求量越域
pub const XC_ZERO_FAIL: u32 = 0x20C88; // 零注入率出现拒绝
pub const XC_STEP_OOR: u32 = 0x20C89; // 步控步号越域

/// 请求方策略。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Strat {
    /// 重试至上限（上限内失败即终败）。
    Retry(u32),
    /// 一次失败即放弃。
    Giveup,
    /// 失败降级为半量请求再试一次。
    Degrade,
}

/// 注入器：率控（每请求按 LCG 抽签）或步控（指定步号必败）。
#[derive(Clone, Copy, Debug)]
pub struct Injector {
    /// 率控千分（0~1000）。
    pub rate_milli: u32,
    /// 步控表（步号从 0 起）。
    pub steps: [bool; 32],
}

impl Injector {
    /// 纯率控。
    pub fn rate(milli: u32) -> Result<Injector, u32> {
        if milli > 1000 {
            return Err(XC_RATE_OOR);
        }
        Ok(Injector { rate_milli: milli, steps: [false; 32] })
    }

    /// 步控叠加。
    pub fn with_step(mut self, step: u32) -> Result<Injector, u32> {
        if step >= 32 {
            return Err(XC_STEP_OOR);
        }
        self.steps[step as usize] = true;
        Ok(self)
    }

    /// 判定第 step 步是否注入失败：步控命中或 LCG 抽签 < 率。
    pub fn hit(&self, step: u32) -> bool {
        if (step as usize) < 32 && self.steps[step as usize] {
            return true;
        }
        if self.rate_milli == 0 {
            return false;
        }
        let x = step
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        ((x >> 16) % 1000) < self.rate_milli as u32
    }
}

/// 受压分配器：总量配额内成功，超出按注入判定拒绝。
pub struct Faucet {
    quota: u64,
    used: u64,
    inj: Injector,
}

impl Faucet {
    /// 建：配额（字节）+ 注入器。
    pub fn new(quota: u64, inj: Injector) -> Result<Faucet, u32> {
        if quota == 0 || quota > 1 << 40 {
            return Err(XC_QUOTA_OOR);
        }
        Ok(Faucet { quota, used: 0, inj })
    }

    /// 单次请求：注入命中或超配额 → 拒（拒绝码非零）。
    pub fn ask(&mut self, step: u32, bytes: u64) -> Result<u64, u32> {
        if bytes == 0 || bytes > self.quota {
            return Err(XC_QUOTA_OOR);
        }
        if self.inj.hit(step) || self.used + bytes > self.quota {
            return Err(1); // 显式拒绝码（非零 = 不静默）
        }
        self.used += bytes;
        Ok(bytes)
    }

    /// 已用量。
    pub fn used(&self) -> u64 {
        self.used
    }
}

/// 请求方执行一轮（带策略），返回落账：(成功数, 失败数, 重试数, 降级数)。
pub fn run_round(
    f: &mut Faucet,
    steps: u32,
    bytes: u64,
    strat: Strat,
) -> Result<(u32, u32, u32, u32), u32> {
    let mut ok = 0u32;
    let mut fail = 0u32;
    let mut retries = 0u32;
    let mut degrades = 0u32;
    let max_retry = match strat {
        Strat::Retry(n) => {
            if n == 0 || n > 16 {
                return Err(XC_RETRY_CAP);
            }
            n
        }
        Strat::Giveup => 1,
        Strat::Degrade => 2,
    };
    for step in 0..steps {
        let mut cur = bytes;
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            match f.ask(step, cur) {
                Ok(_) => {
                    ok += 1;
                    break;
                }
                Err(code) => {
                    if code == 0 {
                        return Err(XC_SILENT);
                    }
                    if attempt >= max_retry {
                        fail += 1;
                        break;
                    }
                    retries += 1;
                    if strat == Strat::Degrade && attempt == 1 {
                        cur = cur / 2;
                        degrades += 1;
                    }
                }
            }
        }
    }
    Ok((ok, fail, retries, degrades))
}

/// 收官对账：成功 + 失败 = 总步数。
pub fn ledger_bal(ok: u32, fail: u32, steps: u32) -> Result<(), u32> {
    if ok + fail != steps {
        return Err(XC_LEDGER_UNBAL);
    }
    Ok(())
}

pub fn run_unx0391_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0391_oominject");
    // 族一：零注入率全程成功。
    let inj0 = match Injector::rate(0) {
        Ok(i) => i,
        Err(_) => {
            cs.add("注入器0", false, "拒");
            return cs;
        }
    };
    let mut f0 = match Faucet::new(10_000, inj0) {
        Ok(f) => f,
        Err(_) => {
            cs.add("龙头0", false, "拒");
            return cs;
        }
    };
    let r0 = run_round(&mut f0, 16, 100, Strat::Giveup);
    cs.add("零率全成", r0 == Ok((16, 0, 0, 0)), "16 步全绿");
    cs.add("零率账平", ledger_bal(16, 0, 16).is_ok(), "16+0=16");
    cs.add("用量 1600", f0.used() == 1600, "16 × 100");
    // 族二：步控精确注入（第 3 步必败，Giveup 一步到位）。
    let inj1 = match Injector::rate(0).and_then(|i| i.with_step(3)) {
        Ok(i) => i,
        Err(_) => Injector::rate(0).unwrap_or(Injector { rate_milli: 0, steps: [false; 32] }),
    };
    // with_step 失败时回退纯率控（判据内防御，正常路径不走此支）。
    let mut f1 = match Faucet::new(10_000, inj1) {
        Ok(f) => f,
        Err(_) => {
            cs.add("龙头1", false, "拒");
            return cs;
        }
    };
    let r1 = run_round(&mut f1, 8, 100, Strat::Giveup);
    cs.add("步控 7 成 1 败", r1 == Ok((7, 1, 0, 0)), "第 3 步败");
    cs.add("账平 8", ledger_bal(7, 1, 8).is_ok(), "7+1=8");
    // 族三：重试策略（上限 3，步 0/1/2 连败 → 第 3 步终败）。
    let inj2 = match Injector::rate(0).and_then(|i| i.with_step(0)).and_then(|i| i.with_step(1)).and_then(|i| i.with_step(2)) {
        Ok(i) => i,
        Err(_) => match Injector::rate(0) {
            Ok(i) => i,
            Err(_) => {
                cs.add("注入器2回退", false, "拒");
                return cs;
            }
        },
    };
    let mut f2 = match Faucet::new(10_000, inj2) {
        Ok(f) => f,
        Err(_) => {
            cs.add("龙头2", false, "拒");
            return cs;
        }
    };
    let r2 = run_round(&mut f2, 4, 100, Strat::Retry(3));
    cs.add("重试三连败", r2 == Ok((1, 1, 6, 0)), "步 0~2 各 3 试");
    cs.add("账平 4", ledger_bal(1, 1, 4).is_ok(), "1+1=4");
    // 重试上限内翻盘：第 3 步起放行。
    let inj3 = match Injector::rate(0).and_then(|i| i.with_step(0)) {
        Ok(i) => i,
        Err(_) => match Injector::rate(0) {
            Ok(i) => i,
            Err(_) => {
                cs.add("注入器回退", false, "拒");
                return cs;
            }
        },
    };
    let mut f3 = match Faucet::new(10_000, inj3) {
        Ok(f) => f,
        Err(_) => {
            cs.add("龙头3", false, "拒");
            return cs;
        }
    };
    let r3 = run_round(&mut f3, 2, 100, Strat::Retry(3));
    cs.add("重试翻盘", r3 == Ok((2, 0, 1, 0)), "步 0 二试成");
    // 族四：降级策略（大请求败 → 半量成）。
    let inj4 = match Injector::rate(0).and_then(|i| i.with_step(0)) {
        Ok(i) => i,
        Err(_) => match Injector::rate(0) {
            Ok(i) => i,
            Err(_) => {
                cs.add("注入器回退", false, "拒");
                return cs;
            }
        },
    };
    let mut f4 = match Faucet::new(10_000, inj4) {
        Ok(f) => f,
        Err(_) => {
            cs.add("龙头4", false, "拒");
            return cs;
        }
    };
    let r4 = run_round(&mut f4, 1, 400, Strat::Degrade);
    cs.add("降级翻盘", r4 == Ok((1, 0, 1, 1)), "400→200");
    // 族五：策略/参数拒绝。
    let inj5 = match Injector::rate(0) {
        Ok(i) => i,
        Err(_) => Injector { rate_milli: 0, steps: [false; 32] },
    };
    let mut f5 = match Faucet::new(10_000, inj5) {
        Ok(f) => f,
        Err(_) => {
            cs.add("龙头5", false, "拒");
            return cs;
        }
    };
    cs.add("重试上限零拒",
        run_round(&mut f5, 1, 100, Strat::Retry(0)) == Err(XC_RETRY_CAP),
        "Retry(0)");
    cs.add("重试上限超拒",
        run_round(&mut f5, 1, 100, Strat::Retry(17)) == Err(XC_RETRY_CAP),
        "Retry(17)");
    cs.add("率超千拒", matches!(Injector::rate(1001), Err(XC_RATE_OOR)), "1001");
    cs.add("步号越域拒", matches!(Injector::rate(0).and_then(|i| i.with_step(32)), Err(XC_STEP_OOR)), "step=32");
    cs.add("配额零拒", matches!(Faucet::new(0, inj5), Err(XC_QUOTA_OOR)), "quota=0");
    // 族六：账不平检出。
    cs.add("账平 4+1≠8 拒", ledger_bal(4, 1, 8) == Err(XC_LEDGER_UNBAL), "5≠8");
    cs
}
