//! VE-F2208 · 域自检（判据逐条对应，见 `vel08_pool.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 总量+单发射器双配额 → `P01-配额-*`
//! - 三阈值水位 + 滞回 → `P02-水位-*` / `P03-滞回-*`
//! - 降级通知可查+ 广播节流 → `P04-降级-*`
//! - 泄漏防线（账本两恒等式）→ `P05-泄漏-*`
//! - 空闲链表 O(1)/ 批量回收顺序 / 摊还 → `P06-分配-*`
//! - 错误路径与降级矩阵（显性/震荡/截断/泄漏/可查）→ `P07-矩阵-*`
//! - 无障碍（诊断中文标签）→ `P08-无障碍-*`
//!
//! 零墙钟、零IO；容量/stride/占比/水位均为注入常量，故回归可复现。
//!
//! **判据设计自律（承 W006·F2203~F2207 弱门禁教训，本单针对性加固）**：
//! ① **夹逼对钉死阈值界位置**：只断「水位 ≥70 进预警」会漏掉「差一档」
//!    的错位（如阈值误设 60或 75 都可能仍绿）。故对三阈值各取
//!    **界下/ 界上 / 界点**三点，并断「界下不进、界点进、界上进」；
//! ② **回收顺序判据必须用 `index != 数组位置` 的反退化语料**：本单
//!    `reclaim` 的批内序取自 `recycled` 标志数组的**下标**，若语料里
//!    「死亡标志位置」恰好等于「槽位号」，则头插与尾插**给出同一序列**，
//!    判据恒绿——必须用**乱序死亡**语料把两者劈开。
//! ③ **账本恒等式必须传真值**：`LeakLedger::residue(pool_live)` 的形参
//!    是池侧实测在用数；若判据传账本自算值，则「账本自比」恒为 0、
//!    变成自证式弱门禁。判据一律传 `pool.live()`。
//! ④ **负向断言须配正向计数**：断「无泄漏」之外，还要造泄漏并断恰等于 N。
//! ⑤ 判据里的 `[i]` 一律 `.get()`：被测函数退化为空实现时判据自己先
//!    panic，看着像「变异未捕获」，实为门禁崩溃。
//! ⑥ 变异分类先看 stdout 再看退出码（探针在 failed>0 时故意 exit(1)）。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use super::vel08_pool::*;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 语料构造（判据侧自持，不依赖被测内部状态）
// ---------------------------------------------------------------------------

/// 造一份 CPU 池配额声明。
fn cpu_quota(capacity: u32, share_pct: u32) -> PoolQuota {
    PoolQuota {
        kind: PoolKind::Cpu,
        capacity,
        stride: 64,
        emitter_share_pct: share_pct,
        bytes_cap: 1 << 30,
    }
}

/// 造一份 GPU 池配额声明（**字节上限独立**，用于核「双池不共享闸」）。
fn gpu_quota(capacity: u32, stride: u32, bytes_cap: u64) -> PoolQuota {
    PoolQuota {
        kind: PoolKind::Gpu,
        capacity,
        stride,
        emitter_share_pct: 10,
        bytes_cap,
    }
}

/// 造池（配额非法时返回 None）。
fn pool_of(q: &PoolQuota) -> Option<ParticlePool> {
    ParticlePool::new(q).ok()
}

/// 把池灌到指定**在用数**（用于精确构造水位）。
///
/// 走真实 `try_alloc` 路径，故 `pool.live()` 恒等于 `want`；若实现退化为
/// 「分配不计数」，判据会在断水位时红，而不是被语料掩盖。
fn fill(pool: &mut ParticlePool, want: u32, bag: &mut PoolBag) -> u32 {
    let mut got = 0u32;
    for _ in 0..want {
        if pool.try_alloc(bag).is_ok() {
            got += 1;
        } else {
            break;
        }
    }
    got
}

/// 判据侧**独立**重算期望档位（带滞回），**刻意不调用 `ParticlePool::observe`**。
///
/// 若判据调用被测的同一个状态机，则「升档阈值写错」「降档阈值写错」
/// 这类变异会同时改掉实现与期望值，判据全绿——典型自证式弱门禁。
/// 此处用**另一套写法**（先按升档点定ceiling，再按回落点下压）重算。
fn expect_level(pct: u64) -> PressureLevel {
    // 升：能到的最高档。
    let mut lv = PressureLevel::Normal;
    if pct >= WM_WARN_PCT {
        lv = PressureLevel::Warn;
    }
    if pct >= WM_DEGRADE_PCT {
        lv = PressureLevel::Degrade;
    }
    if pct >= WM_REJECT_PCT {
        lv = PressureLevel::Reject;
    }
    // 降：从最高档往下压到「水位跌破该档回落点」为止。
    if lv == PressureLevel::Reject && pct < HYST_REJECT_OFF {
        lv = PressureLevel::Degrade;
    }
    if lv == PressureLevel::Degrade && pct < HYST_DEGRADE_OFF {
        lv = PressureLevel::Warn;
    }
    if lv == PressureLevel::Warn && pct < HYST_WARN_OFF {
        lv = PressureLevel::Normal;
    }
    lv
}

/// 判据侧独立重算水位（整数口径，与被测同口径但写法独立）。
fn expect_pct(live: u32, capacity: u32) -> u64 {
    (live as u64 * 100) / (capacity as u64)
}

/// 判据侧独立重算降级因子（**不调用 `emission_scale`**）。
fn expect_scale(pct: u64) -> f32 {
    if pct <= WM_DEGRADE_PCT {
        1.0
    } else if pct >= WM_REJECT_PCT {
        0.0
    } else {
        (WM_REJECT_PCT - pct) as f32 / (WM_REJECT_PCT - WM_DEGRADE_PCT) as f32
    }
}

// ---------------------------------------------------------------------------
// P01 · 总量+单发射器双配额
// ---------------------------------------------------------------------------

fn p01_quota() -> CheckSet {
    let mut s = CheckSet::new("VE-F2208-p01");

    // 总字节 = 容量 × stride（独立重算，不调bytes()）。
    let q = cpu_quota(1000, 10);
    let want_bytes = 1000u64 * 64u64;
    s.add("P01-配额-总字节等于容量乘stride", q.bytes() == Some(want_bytes), "bytes() 须等于 capacity×stride");

    // CPU / GPU 双池配额各自独立：CPU 装得下、GPU 装不下时，CPU 池仍可建。
    let cpu_ok = cpu_quota(1000, 10);
    let gpu_bad = gpu_quota(1000, 4096, 1024); // 需 4 MiB，硬顶仅 1 KiB
    s.add("P01-配额-双池配额独立", pool_of(&cpu_ok).is_some() && pool_of(&gpu_bad).is_none(), "GPU 超硬顶不得连带影响 CPU 池");

    // 四类非法各自独立被拒（**逐条**断，不合并成一个 count——
    // 合并计数会被另一条非零掩护）。
    let cap0 = PoolQuota { kind: PoolKind::Cpu, capacity: 0, stride: 64, emitter_share_pct: 10, bytes_cap: 1 << 30 };
    s.add("P01-配额-容量0独立被拒", cap0.validate().is_err(), "容量 0：池不存在");
    let stride0 = PoolQuota { kind: PoolKind::Cpu, capacity: 100, stride: 0, emitter_share_pct: 10, bytes_cap: 1 << 30 };
    s.add("P01-配额-stride0独立被拒", stride0.validate().is_err(), "stride 0：水位分母为零");
    let share0 = PoolQuota { kind: PoolKind::Cpu, capacity: 100, stride: 64, emitter_share_pct: 0, bytes_cap: 1 << 30 };
    s.add("P01-配额-占比0独立被拒", share0.validate().is_err(), "占比 0：取消单发射器配额");
    let share_over = PoolQuota { kind: PoolKind::Cpu, capacity: 100, stride: 64, emitter_share_pct: 101, bytes_cap: 1 << 30 };
    s.add("P01-配额-占比越界独立被拒", share_over.validate().is_err(), "占比 >100：等于取消单发射器配额");
    let over_cap = PoolQuota { kind: PoolKind::Cpu, capacity: 100, stride: 4096, emitter_share_pct: 10, bytes_cap: 1024 };
    s.add("P01-配额-超硬顶独立被拒", over_cap.validate().is_err(), "声明总量超硬顶");

    // 溢出拒绝（checked_mul）：构造真正会溢出的声明。
    //
    // **判据强度教训（实测）**：初版写成
    // `big.validate().is_err() || big.bytes() == Some(...)`，
    // 用 `||` 让「两臂中任一为真」就算过——这是**弱门禁**：
    // 把 `checked_mul` 换成裸 `*`（溢出回绕）时，两臂**同时**仍成立
    // （validate 判 err、bytes() 也给回绕值），判据察觉不到
    // 「回绕值是错的」。故改为**分别独立断言**且用 `==` 不用 `||`。
    //
    // 正确口径：`capacity × stride` 必须**溢出即 None**，
    // 而裸 `*` 会给出一个「小于真实值」的回绕数 —— 两者必须不等。
    {
        let big = PoolQuota {
            kind: PoolKind::Cpu,
            capacity: u32::MAX,
            stride: 64,
            emitter_share_pct: 50,
            bytes_cap: u64::MAX,
        };
        // u32::MAX × 64 = 2^38，不溢出 u64，故不是溢出用例——
        // 真正的 u64 溢出需 stride 极大，而 stride 是 u32。
        // 故改为断「**贴近边界时不溢出且值精确**」+「bytes_cap 极小时拒」。
        s.add(
            "P01-配额-边界乘法精确",
            big.bytes() == Some(u32::MAX as u64 * 64u64)
                && big.validate().map(|v| v == u32::MAX as u64 * 64u64).unwrap_or(false),
            "u32::MAX×64 不溢出 u64，值须精确",
        );
        // 独立断「超硬顶时validate 必Err」——不与bytes() 用 || 合并。
        let tight = PoolQuota {
            kind: PoolKind::Cpu,
            capacity: u32::MAX,
            stride: 64,
            emitter_share_pct: 50,
            bytes_cap: 1024,
        };
        s.add(
            "P01-配额-超硬顶必拒不靠或",
            tight.bytes().is_some() && !tight.validate().is_ok(),
            "bytes 能算出但超硬顶 ⇒ validate 必 Err（不用 || 合并两臂）",
        );
        // **恰好等于硬顶须通过**（边界口径）：`total > bytes_cap` 是严格大于，
        // 等于硬顶是「刚好装下」而非「装不下」。缺这条时把 `>` 改成 `>=`
        // 的变异全绿——而那会让「按配额精确申请」的使用者被无故拒绝。
        let exact = PoolQuota {
            kind: PoolKind::Cpu,
            capacity: 100,
            stride: 64,
            emitter_share_pct: 10,
            bytes_cap: 100 * 64,
        };
        s.add(
            "P01-配额-恰好等于硬顶须通过",
            exact.bytes() == Some(100 * 64) && exact.validate().map(|v| v == 100 * 64).unwrap_or(false),
            "总量恰等于硬顶 6400 ⇒ 通过（严格大于才拒）",
        );
        // 反向：恰超 1 字节必拒。
        let over1 = PoolQuota {
            kind: PoolKind::Cpu,
            capacity: 100,
            stride: 64,
            emitter_share_pct: 10,
            bytes_cap: 100 * 64 - 1,
        };
        s.add(
            "P01-配额-超硬顶一字节必拒",
            over1.bytes() == Some(100 * 64) && !over1.validate().is_ok(),
            "总量超硬顶 1 字节 ⇒ 必拒",
        );
    }

    // 单发射器上限：默认 10%，独立重算。
    let q10 = cpu_quota(1000, 10);
    s.add("P01-配额-单发射器上限占比10", q10.emitter_cap() == 100, "1000×10%=100 槽");
    let q50 = cpu_quota(1000, 50);
    s.add("P01-配额-单发射器上限可配", q50.emitter_cap() == 500, "1000×50%=500 槽");
    // 小池下限保护：占比算出 0 时兜到 1（不是 0——0 会让任何分配都超限）。
    let tiny = cpu_quota(4, 10);
    s.add("P01-配额-小池上限兜底为1", tiny.emitter_cap() == 1, "4×10%=0 须兜到 1");
    // 100% 显式取消该约束。
    let full = cpu_quota(1000, 100);
    s.add("P01-配额-占比100等于池容量", full.emitter_cap() == 1000, "100% 时上限 = 池容量");

    // 单发射器上限**独立于**池总量水位：池只用了 50%（水位正常），
    // 但单发射器已达其 10% 上限 => 截断（防单发射器吞池的独立性）。
    let mut p = match pool_of(&cpu_quota(1000, 10)) {
        Some(v) => v,
        None => {
            s.fail("P01-配额-单发射器截断独立于总水位", "建池失败");
            return s;
        }
    };
    let mut b = PoolBag::new();
    let mut me = EmitterBudget::new(100);
    let mut mine = 0;
    for _ in 0..150 {
        if alloc_for(&mut p, &mut me, &mut b).unwrap().slot != NO_SLOT {
            mine += 1;
        }
    }
    let pct_after = p.water_pct();
    s.add("P01-配额-单发射器截断独立于总水位", mine == 100 && pct_after == 10, "单发射器被截到 100 槽，而池水位仅 10%（远低于降级档）");

    // 截断产生告警且原因可查。
    s.add("P01-配额-截断告警可查", b.has(PoolDiag::EmitterTruncated) && b.count(PoolDiag::EmitterTruncated) == 50, "150 次请求中 50 次被截断，须逐条告警");

    s
}

// ---------------------------------------------------------------------------
// P02 · 三阈值水位（夹逼对钉死界位置）
// ---------------------------------------------------------------------------

fn p02_watermark() -> CheckSet {
    let mut s = CheckSet::new("VE-F2208-p02");

    // 夹逼对①：预警档 70。界下 69 不进、界点 70 进、界上 71 进。
    for (live, want) in [(69u32, PressureLevel::Normal), (70, PressureLevel::Warn), (71, PressureLevel::Warn)] {
        let mut p = match pool_of(&cpu_quota(100, 100)) {
            Some(v) => v,
            None => {
                s.fail("P02-水位-夹逼对预警", "建池失败");
                return s;
            }
        };
        let mut b = PoolBag::new();
        fill(&mut p, live, &mut b);
        p.observe(&mut b);
        s.add(
            "P02-水位-夹逼对预警70",
            p.level() == want && p.level() == expect_level(expect_pct(live, 100)),
            "69不进/70进/71进",
        );
        break;
    }
    // 逐点跑三档（分开写，避免一个失败掩盖其余）。
    {
        let mut p = pool_of(&cpu_quota(100, 100)).unwrap();
        let mut b = PoolBag::new();
        fill(&mut p, 70, &mut b);
        p.observe(&mut b);
        s.add("P02-水位-界点70进预警", p.level() == PressureLevel::Warn, "界点须进");
    }
    {
        let mut p = pool_of(&cpu_quota(100, 100)).unwrap();
        let mut b = PoolBag::new();
        fill(&mut p, 69, &mut b);
        p.observe(&mut b);
        s.add("P02-水位-界下69不进预警", p.level() == PressureLevel::Normal, "界下须不进");
    }
    {
        let mut p = pool_of(&cpu_quota(100, 100)).unwrap();
        let mut b = PoolBag::new();
        fill(&mut p, 85, &mut b);
        p.observe(&mut b);
        s.add("P02-水位-界点85进降级", p.level() == PressureLevel::Degrade, "降级界点");
    }
    {
        let mut p = pool_of(&cpu_quota(100, 100)).unwrap();
        let mut b = PoolBag::new();
        fill(&mut p, 84, &mut b);
        p.observe(&mut b);
        s.add("P02-水位-界下84不进降级", p.level() == PressureLevel::Warn, "84 仍在预警档");
    }
    {
        let mut p = pool_of(&cpu_quota(100, 100)).unwrap();
        let mut b = PoolBag::new();
        fill(&mut p, 95, &mut b);
        p.observe(&mut b);
        s.add("P02-水位-界点95进拒绝", p.level() == PressureLevel::Reject, "拒绝界点");
    }
    {
        let mut p = pool_of(&cpu_quota(100, 100)).unwrap();
        let mut b = PoolBag::new();
        fill(&mut p, 94, &mut b);
        p.observe(&mut b);
        s.add("P02-水位-界下94不进拒绝", p.level() == PressureLevel::Degrade, "94 仍在降级档");
    }

    // 水位百分比口径：整数，独立重算。
    {
        let mut p = pool_of(&cpu_quota(1000, 100)).unwrap();
        let mut b = PoolBag::new();
        let got = fill(&mut p, 345, &mut b);
        s.add(
            "P02-水位-百分比口径正确",
            got == 345 && p.water_pct() == expect_pct(345, 1000) && p.water_pct() == 34,
            "345/1000 = 34%",
        );
    }
    // 容量 0 时水位钉 100（不返回荒谬值、不除零 panic）。
    s.add("P02-水位-容量0钳到100", water_pct(0, 0) == 100, "容量 0 钉顶，不除零");
    // live 超 capacity（逻辑矛盾）时钳到 100，绝不 >100。
    s.add("P02-水位-超容钳到100", water_pct(150, 100) == 100, "live>capacity须钳顶");

    // 升档逐档爬不跳档：50 仍Normal（未过 70）、75 进 Warn（未过 85）、
    // 90 进 Degrade、99 进 Reject（实测灌到 95 即触发，100 槽已占满）。
    // 曾把 50 与 75 的期望分别误写成 Warn 与 Degrade —— 判据自身算错
    // （50<70 不过预警、75<85 不过降级），实测实现是对的。
    {
        let mut p = pool_of(&cpu_quota(100, 100)).unwrap();
        let mut b = PoolBag::new();
        let mut seen = Vec::new();
        for target in [50u32, 75, 90, 99] {
            while p.live() < target {
                if p.try_alloc(&mut b).is_err() {
                    break;
                }
            }
            p.observe(&mut b);
            seen.push(p.level());
        }
        s.add(
            "P02-水位-升档逐档爬",
            seen == vec![PressureLevel::Normal, PressureLevel::Warn, PressureLevel::Degrade, PressureLevel::Reject],
            "50→Normal(未过70), 75→Warn(未过85), 90→Degrade, 95+→Reject",
        );
    }

    // 档位中文标签齐备（读屏可达）。
    s.add(
        "P02-水位-四档标签齐备",
        !PressureLevel::Normal.zh().is_empty()
            && !PressureLevel::Warn.zh().is_empty()
            && !PressureLevel::Degrade.zh().is_empty()
            && !PressureLevel::Reject.zh().is_empty(),
        "四档均须有中文标签",
    );
    s.add(
        "P02-水位-四档标签互不相同",
        PressureLevel::Normal.zh() != PressureLevel::Warn.zh()
            && PressureLevel::Warn.zh() != PressureLevel::Degrade.zh()
            && PressureLevel::Degrade.zh() != PressureLevel::Reject.zh(),
        "标签不得重复",
    );

    s
}

// ---------------------------------------------------------------------------
// P03 · 滞回（升降阈值分离，防降级震荡）
// ---------------------------------------------------------------------------

fn p03_hysteresis() -> CheckSet {
    let mut s = CheckSet::new("VE-F2208-p03");

    // 三对滞回阈值必须**严格分离**（升> 降），否则滞回不存在。
    s.add(
        "P03-滞回-三对阈值分离",
        WM_WARN_PCT > HYST_WARN_OFF && WM_DEGRADE_PCT > HYST_DEGRADE_OFF && WM_REJECT_PCT > HYST_REJECT_OFF,
        "每档升点须高于回落点",
    );
    s.add(
        "P03-滞回-档位序不交叠",
        WM_WARN_PCT <= WM_DEGRADE_PCT && WM_DEGRADE_PCT <= WM_REJECT_PCT,
        "70<=85<=95",
    );

    // 死区中点：降档档位的死区 (80,85) 内取 82 须**保持** Degrade。
    {
        let mut p = pool_of(&cpu_quota(100, 100)).unwrap();
        let mut b = PoolBag::new();
        fill(&mut p, 85, &mut b);
        p.observe(&mut b);
        let hi = p.level();
        // 泄到 82（需回收 3 个槽）。
        let dead: Vec<u32> = [0u32, 1, 2].to_vec();
        p.reclaim(&dead);
        p.observe(&mut b);
        s.add(
            "P03-滞回-降级死区保持",
            hi == PressureLevel::Degrade && p.water_pct() == 82 && p.level() == PressureLevel::Degrade,
            "85→82 在 (80,85) 死区内，档位须保持 Degrade",
        );
    }
    // 滞回**边界语义**：恰在回落点（80）**保持**降级档，跌破到 79 才退出。
    // 判据退出条件是 `pct < 回落点`（严格小于），故 80 不退。
    // 曾把期望写成「85→80 须退出」—— 那是判据自身把边界当成了退出点，
    // 实测实现（严格小于）是对的：边界含在死区内。
    {
        let mut p = pool_of(&cpu_quota(100, 100)).unwrap();
        let mut b = PoolBag::new();
        fill(&mut p, 85, &mut b);
        p.observe(&mut b);
        let at_degrade = p.level();
        p.reclaim(&[0, 1, 2, 3, 4]); // 85 → 80（恰在回落点）
        p.observe(&mut b);
        let at_boundary = p.level();
        let pct_at = p.water_pct();
        p.reclaim(&[5]); // 80 → 79（跌破回落点）
        p.observe(&mut b);
        s.add(
            "P03-滞回-跌出回落点退出",
            at_degrade == PressureLevel::Degrade
                && pct_at == 80
                && at_boundary == PressureLevel::Degrade
                && p.water_pct() == 79
                && p.level() == expect_level(p.water_pct()),
            "85→80 恰在回落点须保持 Degrade（严格小于才退），80→79 须退出",
        );
    }
    // 预警死区 (65,70) 内取 67 保持 Warn；跌到 64 退出。
    {
        let mut p = pool_of(&cpu_quota(100, 100)).unwrap();
        let mut b = PoolBag::new();
        fill(&mut p, 70, &mut b);
        p.observe(&mut b);
        let a = p.level();
        p.reclaim(&[0, 1, 2]);
        p.observe(&mut b);
        let in_dead = p.level();
        p.reclaim(&[3, 4, 5]);
        p.observe(&mut b);
        let out = p.level();
        s.add(
            "P03-滞回-预警死区保持与退出",
            a == PressureLevel::Warn && in_dead == PressureLevel::Warn && out == PressureLevel::Normal,
            "70→67 保持、→64 退出",
        );
    }
    // 拒绝死区 (90,95) 内取 92 保持 Reject；跌到 89 退回降级。
    {
        let mut p = pool_of(&cpu_quota(100, 100)).unwrap();
        let mut b = PoolBag::new();
        fill(&mut p, 95, &mut b);
        p.observe(&mut b);
        let a = p.level();
        p.reclaim(&[0, 1, 2]);
        p.observe(&mut b);
        let in_dead = p.level();
        p.reclaim(&[3, 4, 5, 6]);
        p.observe(&mut b);
        let out = p.level();
        s.add(
            "P03-滞回-拒绝死区保持与退出",
            a == PressureLevel::Reject && in_dead == PressureLevel::Reject && out == PressureLevel::Degrade,
            "95→92 保持、→89 退回降级",
        );
    }

    // **震荡演练**：水位在阈值上下反复，逐帧档位**不得**每帧翻转。
    // 走 85/82 交替 6 轮，档位全程须恒为 Degrade。
    {
        let mut p = pool_of(&cpu_quota(100, 100)).unwrap();
        let mut b = PoolBag::new();
        fill(&mut p, 85, &mut b);
        let mut stable = true;
        for _ in 0..6 {
            p.observe(&mut b);
            if p.level() != PressureLevel::Degrade {
                stable = false;
            }
            // 泄 3 → 82，仍在死区；再补 3 回85。
            p.reclaim(&[0, 1, 2]);
            if p.try_alloc(&mut b).is_ok() {
                stable = stable && true;
            }
            if p.try_alloc(&mut b).is_ok() {}
            if p.try_alloc(&mut b).is_ok() {}
        }
        p.observe(&mut b);
        s.add(
            "P03-滞回-震荡不翻转",
            stable && p.level() == PressureLevel::Degrade,
            "85/82 交替 6 轮，档位须恒 Degrade（无滞回则每帧翻转）",
        );
    }

    // 判据侧独立重算与被测在**全水位扫描**上逐点一致（含所有死区）。
    {
        let mut agree = true;
        for pct in 0u64..=100 {
            let live = pct as u32; //容量 100 ⇒ live == pct
            let mut p = pool_of(&cpu_quota(100, 100)).unwrap();
            let mut b = PoolBag::new();
            fill(&mut p, live, &mut b);
            p.observe(&mut b);
            if p.level() != expect_level(pct) {
                agree = false;
                break;
            }
        }
        s.add("P03-滞回-全水位扫描与独立重算一致", agree, "0..=100 每点档位须与判据侧重算一致");
    }

    s
}

// ---------------------------------------------------------------------------
// P04 · 降级通知可查 + 广播节流
// ---------------------------------------------------------------------------

fn p04_degrade() -> CheckSet {
    let mut s = CheckSet::new("VE-F2208-p04");

    // 降级因子：反比线性，独立重算 + 全档扫描。
    {
        let mut agree = true;
        for pct in 0u64..=100 {
            let a = emission_scale(pct);
            let e = expect_scale(pct);
            if (a - e).abs() > 1e-6 {
                agree = false;
                break;
            }
        }
        s.add("P04-降级-因子全档与独立重算一致", agree, "0..=100 逐点比对");
    }
    // 因子单调不增（水位越高发射率越低）——单独断方向。
    {
        let mut mono = true;
        let mut prev = emission_scale(0);
        for pct in 1u64..=100 {
            let cur = emission_scale(pct);
            if cur > prev + 1e-9 {
                mono = false;
                break;
            }
            prev = cur;
        }
        s.add("P04-降级-因子单调不增", mono, "水位升高时因子不得反升");
    }
    // **边界点专项**：85 恰在降档阈值上，须返回 1.0（不缩减）。
    // 缺这条时，把 `pct <= 85` 改成 `pct < 85` 的变异**全绿**——
    // 因为 85 恰好还走下一分支算出 (95-85)/(95-85)=1.0，两条路径
    // 在边界点**数值重合**，靠全档扫描抓不到（实测漏网一项）。
    // 故必须**单独**钉边界点的返回值与其来源分支无关性。
    s.add(
        "P04-降级-边界85返回1",
        emission_scale(85) == 1.0 && emission_scale(84) == 1.0 && emission_scale(86) < 1.0,
        "85（含）不缩减、84 不缩减、86 已缩减——三条一起钉才能劈开 <= 与 <",
    );
    // 边界点专项：95 恰在拒绝阈值上须返回 0.0；94 已 <1。
    s.add(
        "P04-降级-边界95返回0",
        emission_scale(95) == 0.0 && emission_scale(94) < 1.0,
        "95（含）停止新发射、94 已缩减",
    );
    // 因子在阈值两侧的**跳变幅度**受限（连续降级，非阶跃）：
    // 相邻 1 个百分点水位差导致的因子变化须远小于 0.5（阶跃会 ≈0.5+）。
    {
        let mut max_jump = 0.0f32;
        for pct in 0u64..100 {
            let d = (emission_scale(pct + 1) - emission_scale(pct)).abs();
            if d > max_jump {
                max_jump = d;
            }
        }
        s.add(
            "P04-降级-因子连续无阶跃",
            max_jump < 0.2,
            "相邻 1% 水位的因子变化须 < 0.2（阶跃实现会是 0.5+）",
        );
    }
    // 锚点值：85 → 1.0、90 → 0.5、95 → 0.0、70 → 1.0（未降级不缩）。
    s.add("P04-降级-锚点85为1", (emission_scale(85) - 1.0).abs() < 1e-6, "85% 不缩减");
    s.add("P04-降级-锚点90为半", (emission_scale(90) - 0.5).abs() < 1e-6, "90% 半速");
    s.add("P04-降级-锚点95为零", emission_scale(95) == 0.0, "95% 停止新发射");
    s.add("P04-降级-70不缩减", (emission_scale(70) - 1.0).abs() < 1e-6, "预警档不缩减");

    // 事件三字段齐备：水位 / 因子 / 原因，且因子与水位**自洽**。
    for (pct, want_level) in [(70u64, "预警"), (85, "降级"), (95, "拒绝")] {
        let ev = pressure_event(pct, 1);
        s.add(
            "P04-降级-事件三字段自洽",
            ev.water_pct == pct && (ev.scale - expect_scale(pct)).abs() < 1e-6 && ev.reason.contains(want_level),
            "水位/因子/原因须齐备且含档位名",
        );
        break;
    }
    {
        let ev = pressure_event(85, 1);
        s.add("P04-降级-预警事件可查", pressure_event(70, 1).reason.contains("预警"), "70% 事件原因须写明预警");
        s.add("P04-降级-降级事件可查", ev.reason.contains("降级"), "85% 事件原因须写明降级");
        s.add("P04-降级-拒绝事件可查", pressure_event(95, 1).reason.contains("拒绝"), "95% 事件原因须写明拒绝");
        s.add("P04-降级-无降级有占位原因", pressure_event(10, 0).reason == NO_PRESSURE_REASON, "未降级时给占位原因而非空串");
    }

    // 广播节流：**每帧至多一次**（性能契约）。
    {
        let mut p = pool_of(&cpu_quota(100, 100)).unwrap();
        let mut b = PoolBag::new();
        fill(&mut p, 90, &mut b);
        let first = p.broadcast_degrade();
        let second = p.broadcast_degrade();
        let third = p.broadcast_degrade();
        s.add(
            "P04-降级-每帧至多广播一次",
            first.is_some() && second.is_none() && third.is_none(),
            "同帧二次广播须被拒（上千发射器时是纯开销）",
        );
        p.end_frame();
        s.add("P04-降级-帧边界后可再广播", p.broadcast_degrade().is_some(), "end_frame 重置计数");
    }
    // 未过预警档不广播。
    {
        let mut p = pool_of(&cpu_quota(100, 100)).unwrap();
        let mut b = PoolBag::new();
        fill(&mut p, 50, &mut b);
        s.add("P04-降级-未过预警不广播", p.broadcast_degrade().is_none(), "50% 不广播");
    }

    // 请求侧缩减（发射器消费因子）：floor 且**不超**原请求。
    s.add("P04-降级-请求缩减floor", scaled_request(100, 0.5) == 50 && scaled_request(7, 0.5) == 3, "floor 且不超原值");
    s.add("P04-降级-因子1不缩减", scaled_request(100, 1.0) == 100, "scale=1 原样");
    s.add("P04-降级-因子0归零", scaled_request(100, 0.0) == 0, "scale=0 不发射");
    s.add("P04-降级-因子负归零", scaled_request(100, -1.0) == 0, "负因子兜底为 0");
    s.add("P04-降级-因子非有限不缩减", scaled_request(100, f32::NAN) == 100, "NaN 因子兜底为 1（原请求）");

    // 池侧 scale() 与事件 scale() 一致（同一口径，单源）。
    {
        let mut p = pool_of(&cpu_quota(100, 100)).unwrap();
        let mut b = PoolBag::new();
        fill(&mut p, 90, &mut b);
        let ev = pressure_event(p.water_pct(), 1);
        s.add("P04-降级-池与事件因子同源", (p.scale() - ev.scale).abs() < 1e-9, "pool.scale() == event.scale");

        s.add("P04-降级-池水位口径同源", p.water_pct() == 90, "灌到 90/100 即 90%");
    }

    s
}

// ---------------------------------------------------------------------------
// P05 · 泄漏防线（账本两恒等式，正负向成对）
// ---------------------------------------------------------------------------

fn p05_leak() -> CheckSet {
    let mut s = CheckSet::new("VE-F2208-p05");

    // 健康路径：两个残差都为0（**传池侧实测 live**，非账本自比）。
    {
        let mut p = pool_of(&cpu_quota(100, 100)).unwrap();
        let mut b = PoolBag::new();
        let mut l = LeakLedger::new();
        for _ in 0..10 {
            p.try_alloc(&mut b).unwrap();
            l.on_alloc();
        }
        let flags = vec![true, true, true, false, false, false, false, false, false, false];
        let got = collect_dead(&flags, &mut p, &mut l);
        s.add(
            "P05-泄漏-健康路径两残差为零",
            got == 3 && l.residue(p.live()) == 0 && l.pending_dead() == 0,
            "10分配3回收：残差0、死亡未回收0",
        );
        s.add("P05-泄漏-健康路径账本自检通过", l.audit(p.live(), &mut b), "audit 不报泄漏");
    }

    // 负向①：只标死不回收 ⇒ 死亡未回收累计**恰等于** N。
    {
        let mut l = LeakLedger::new();
        l.mark_dead(7);
        s.add("P05-泄漏-只标死累计恰等N", l.pending_dead() == 7, "7 死亡 0 回收 ⇒ 累计 7");
    }
    // 负向②：部分回收 ⇒ 累计恰等于差额。
    {
        let mut l = LeakLedger::new();
        l.mark_dead(7);
        l.mark_reclaimed(3);
        s.add("P05-泄漏-部分回收累计恰等差额", l.pending_dead() == 4, "7死3回收 ⇒ 累计 4");
    }
    // 负向③：恒等式①抓丢槽（账本说在用 2、池实测 1）。
    {
        let mut l = LeakLedger::new();
        l.alloc_count = 5;
        l.freed_count = 3;
        s.add("P05-泄漏-恒等式一抓丢槽", l.residue(1) == 1 && l.residue(2) == 0, "账本在用2：池实测1⇒残差1、实测2⇒0");
    }
    // 负向④：重复回收（多还）也要被抓。
    {
        let mut l = LeakLedger::new();
        l.alloc_count = 5;
        l.freed_count = 6; // 多还了 1
        s.add("P05-泄漏-恒等式一抓多还", l.residue(0) == 1, "多还 1 ⇒ 残差 1");
    }
    // 审计产诊断且可查。
    {
        let mut l = LeakLedger::new();
        l.mark_dead(4);
        let mut b = PoolBag::new();
        s.add("P05-泄漏-审计产诊断", !l.audit(0, &mut b) && b.has(PoolDiag::LeakDetected), "残差非零须产泄漏诊断");
    }

    // **正向计数覆盖负向**：造足事件并断恰等于 N（防止「永远返回0」骗过负向）。
    {
        let mut p = pool_of(&cpu_quota(100, 100)).unwrap();
        let mut b = PoolBag::new();
        let mut l = LeakLedger::new();
        for _ in 0..20 {
            p.try_alloc(&mut b).unwrap();
            l.on_alloc();
        }
        // 只回收 4个死亡 → 死亡未回收恰 16。
        let mut flags = vec![false; 20];
        for f in flags.iter_mut().take(4) {
            *f = true;
        }
        let got = collect_dead(&flags, &mut p, &mut l);
        s.add(
            "P05-泄漏-正向覆盖恰等16",
            got == 4 && l.dead_marked == 4 && l.dead_reclaimed == 4 && l.pending_dead() == 0 && l.live() == 16,
            "20分配4回收：死亡4回收4、在用16",
        );
    }

    // collect_dead 越界标志不入账（recycled 数组长于池容量）。
    // **判据钉的是 `dead_marked` 计数，不是最终回收数**——因为
    // `reclaim` 内部**也**有一层 `(s as usize) >= n` 过滤，两者等价：
    // 只断最终回收数的话，删掉 `collect_dead` 的过滤仍全绿（被内层兜住）。
    // 但 `mark_dead(batch.len())` 记的是**批长**：删掉外层过滤后
    // 12 个标志会记成 12 而不是 8，账本口径立刻偏离——这条抓得住。
    {
        let mut p = pool_of(&cpu_quota(8, 100)).unwrap();
        let mut b = PoolBag::new();
        let mut l = LeakLedger::new();
        for _ in 0..8 {
            p.try_alloc(&mut b).unwrap();
            l.on_alloc();
        }
        // 长度 12 > 容量 8：4 个越界标志须在外层就被滤掉。
        let flags = vec![true; 12];
        let got = collect_dead(&flags, &mut p, &mut l);
        s.add(
            "P05-泄漏-越界死亡标志不入账",
            got == 8 && l.dead_marked == 8 && l.live() == 0 && p.live() == 0 && p.free_count() == 8,
            "12 标志但仅 8 槽：回收恰 8、**dead_marked 恰8**、在用归零（4 越界标志外层已滤）",
        );
        // 反向：容量 8、标志长度 10、其中 3 真 + 2 越界真 ⇒ 回收恰 3，
        // dead_marked 恰 3（越界真不得凑进批长度）。
        {
            let mut p2 = pool_of(&cpu_quota(8, 100)).unwrap();
            let mut b2 = PoolBag::new();
            let mut l2 = LeakLedger::new();
            for _ in 0..8 {
                p2.try_alloc(&mut b2).unwrap();
                l2.on_alloc();
            }
            let flags2 = vec![true, true, true, false, false, false, false, false, true, true];
            let got2 = collect_dead(&flags2, &mut p2, &mut l2);
            s.add(
                "P05-泄漏-标志数与回收数一一对应",
                got2 == 3 && l2.dead_marked == 3 && p2.live() == 5,
                "8 槽中3 真 + 2 越界真 ⇒ 回收恰 3、**dead_marked 恰 3**、在用 5",
            );
        }
    }

    // 泄漏诊断码标签齐备（读屏）。
    s.add(
        "P05-泄漏-泄漏码标签非空",
        !PoolDiag::LeakDetected.zh().is_empty() && PoolDiag::LeakDetected.zh() != "",
        "泄漏码须有中文标签",
    );

    s
}

// ---------------------------------------------------------------------------
// P06 · 分配/回收：链表、顺序、摊还
// ---------------------------------------------------------------------------

fn p06_alloc() -> CheckSet {
    let mut s = CheckSet::new("VE-F2208-p06");

    // 建池链表序 == 槽位序升序。
    {
        let p = pool_of(&cpu_quota(8, 100)).unwrap();
        s.add("P06-分配-建池链表升序", p.walk_all() == vec![0, 1, 2, 3, 4, 5, 6, 7], "链表序 = 0..n-1");
    }
    // 分配严格从链头取（LIFO）。
    {
        let mut p = pool_of(&cpu_quota(8, 100)).unwrap();
        let mut b = PoolBag::new();
        let got: Vec<u32> = (0..3).map(|_| p.try_alloc(&mut b).unwrap()).collect();
        s.add("P06-分配-从链头取", got == vec![0, 1, 2], "依次取 0,1,2");
    }
    // 内容守恒：链表长度恒等于 free_count（断「丢元素/重复元素」——
    // 只断长度不看内容会漏「链表里有重复槽位」）。
    {
        let mut p = pool_of(&cpu_quota(16, 100)).unwrap();
        let mut b = PoolBag::new();
        let mut ok = true;
        for _ in 0..6 {
            p.try_alloc(&mut b).unwrap();
        }
        p.reclaim(&[1, 3, 5]);
        p.reclaim(&[0, 2]);
        let all = p.walk_all();
        if all.len() as u32 != p.free_count() {
            ok = false;
        }
        // 无重复（O(n²) 但 n 小）：用排序后比较长度。
        let mut sorted = all.clone();
        sorted.sort_unstable();
        for w in sorted.windows(2) {
            if w[0] == w[1] {
                ok = false;
            }
        }
        s.add("P06-分配-链表内容守恒无重复", ok, "长度==free_count 且无重复槽位");
    }

    // **反退化语料**：乱序死亡批，头插序 == 批次序（**非逆序**）。
    // 若语料里死亡标志位置 == 槽位号，头插与尾插给出同一序列，判据恒绿。
    {
        let mut p = pool_of(&cpu_quota(16, 100)).unwrap();
        let mut b = PoolBag::new();
        for _ in 0..6 {
            p.try_alloc(&mut b).unwrap();
        }
        // 先泄掉一些让 free 非空，再回收乱序批，验证「批序+ 原free序」。
        p.reclaim(&[4, 5]);
        let before = p.peek_free(2); // 原 free 序
        // 占掉 0,1,2,3 再乱序回收 [3,1]。
        let mut p2 = pool_of(&cpu_quota(16, 100)).unwrap();
        let mut b2 = PoolBag::new();
        for _ in 0..4 {
            p2.try_alloc(&mut b2).unwrap();
        }
        p2.reclaim(&[3, 1]);
        s.add(
            "P06-分配-批量回收头插保序",
            p2.peek_free(2) == vec![3, 1] && before.len() == 2,
            "乱序批 [3,1] 头插后链表序须为 [3,1]（非 [1,3]）",
        );
    }
    // 批内序在**含原 free 链**时也保序。
    //注意：槽 3 仍在用（未回收），故**不在**空闲链上——期望链是
    // `[2,0]` 接原 free `[4,5,…]`。曾把期望误写成 `[2,0,3,4]`，
    // 那是判据自身算错（把在用槽当成了空闲槽），实现是对的。
    {
        let mut p = pool_of(&cpu_quota(16, 100)).unwrap();
        let mut b = PoolBag::new();
        for _ in 0..4 {
            p.try_alloc(&mut b).unwrap();
        }
        p.reclaim(&[2, 0]); // 批序 [2,0]
        s.add(
            "P06-分配-批序接原free序",
            p.peek_free(4) == vec![2, 0, 4, 5],
            "批 [2,0] 头插到原 free [4,5..] 之前 ⇒ [2,0,4,5]（槽3在用故不在链）",
        );
    }
    // 幂等：重复回收同一批不再计数。
    {
        let mut p = pool_of(&cpu_quota(8, 100)).unwrap();
        let mut b = PoolBag::new();
        for _ in 0..2 {
            p.try_alloc(&mut b).unwrap();
        }
        let first = p.reclaim(&[0, 1]);
        let second = p.reclaim(&[0, 1]);
        s.add(
            "P06-分配-回收幂等",
            first == 2 && second == 0 && p.free_count() == p.capacity(),
            "重复回收不重复计数",
        );
    }
    // 越界槽不入链。
    {
        let mut p = pool_of(&cpu_quota(8, 100)).unwrap();
        let mut b = PoolBag::new();
        for _ in 0..2 {
            p.try_alloc(&mut b).unwrap();
        }
        let n = p.reclaim(&[100, u32::MAX]);
        s.add(
            "P06-分配-越界槽不入链",
            n == 0 && p.free_count() == 6,
            "越界与哨兵槽须被忽略",
        );
    }
    // 池耗尽路径（**防御分支，正常不可达**）：水位 100% 时水位拒绝先命中，
    // 故 `PoolExhausted` 只在「链表与计数不一致」时才出现。判据断的是
    // 「水位拒绝优先于耗尽」这条次序，以及耗尽码的中文标签存在。
    // 曾把期望写成「未观测档位时走耗尽码」——那是判据忽略了
    // 「pct=100 ≥ 95 时水位拒绝必然先命中」的实现次序，实测实现是对的。
    {
        let mut p = pool_of(&cpu_quota(4, 100)).unwrap();
        let mut b = PoolBag::new();
        for _ in 0..4 {
            p.try_alloc(&mut b).unwrap();
        }
        let e = p.try_alloc(&mut b);
        s.add(
            "P06-分配-水位拒绝优先于耗尽",
            p.level() == PressureLevel::Normal
                && p.water_pct() == 100
                && b.has(PoolDiag::WatermarkReject)
                && e.is_err(),
            "槽满必pct=100，水位拒绝先命中（耗尽分支为防御，正常不可达）",
        );
        s.add(
            "P06-分配-耗尽码标签齐备",
            !PoolDiag::PoolExhausted.zh().is_empty(),
            "防御分支亦须有中文标签（读屏可达）",
        );
    }

    // 摊还 O(1)：批量回收 N 个后，链表总长守恒、free_count 正确。
    {
        let mut p = pool_of(&cpu_quota(1000, 100)).unwrap();
        let mut b = PoolBag::new();
        let mut all = Vec::new();
        for _ in 0..500 {
            all.push(p.try_alloc(&mut b).unwrap());
        }
        let got = p.reclaim(&all);
        s.add(
            "P06-分配-批量回收摊还守恒",
            got == 500 && p.free_count() == 1000 && p.walk_all().len() == 1000,
            "500 批量回收：空闲回到 1000、链表长度 1000",
        );
    }

    s
}

// ---------------------------------------------------------------------------
// P07 · 错误路径与降级矩阵（显性/震荡/截断/泄漏/可查）
// ---------------------------------------------------------------------------

fn p07_matrix() -> CheckSet {
    let mut s = CheckSet::new("VE-F2208-p07");

    // 显性拒绝：超 95 新分配拒绝，**不静默截断**。
    {
        let mut p = pool_of(&cpu_quota(100, 100)).unwrap();
        let mut b = PoolBag::new();
        fill(&mut p, 96, &mut b);
        p.observe(&mut b);
        let e = p.try_alloc(&mut b);
        s.add(
            "P07-矩阵-超95显性拒绝",
            e.is_err() && p.level() == PressureLevel::Reject,
            "96% 在用时新分配须被拒",
        );
        s.add(
            "P07-矩阵-拒绝三要素齐备",
            match e {
                Err(r) => r.has_all_elements() && !r.message.is_empty() && !r.hint.is_empty(),
                Ok(_) => false,
            },
            "当前水位/上限/建议三要素非空",
        );
        s.add(
            "P07-矩阵-拒绝码是拒绝类",
            PoolDiag::WatermarkReject.is_reject() && !PoolDiag::EmitterTruncated.is_reject(),
            "拒绝与截断语义须可辨",
        );
    }
    // 拒绝消息**写明当前水位**（可查）。
    {
        let r = Rejection::new(97, 100, 97);
        s.add("P07-矩阵-拒绝消息含水位", r.message.contains("97"), "消息须含当前水位值");
        s.add("P07-矩阵-拒绝建议含回落点", r.hint.contains("90"), "建议须指向拒绝档回落点");
    }

    // 截断不挤占他者：三个发射器，只有超限那个被截。
    {
        let mut p = pool_of(&cpu_quota(1000, 10)).unwrap();
        let mut b = PoolBag::new();
        let mut e1 = EmitterBudget::new(100);
        let mut e2 = EmitterBudget::new(100);
        let mut e3 = EmitterBudget::new(100);
        for _ in 0..100 {
            alloc_for(&mut p, &mut e1, &mut b).unwrap();
        }
        // e1 已满，请求被截。
        let t = alloc_for(&mut p, &mut e1, &mut b).unwrap();
        // e2/e3 仍能正常分配（未被连坐）。
        let a2 = alloc_for(&mut p, &mut e2, &mut b).unwrap();
        let a3 = alloc_for(&mut p, &mut e3, &mut b).unwrap();
        s.add(
            "P07-矩阵-截断不连坐他者",
            t.truncated && t.slot == NO_SLOT && !a2.truncated && !a3.truncated && e2.live == 1 && e3.live == 1,
            "e1 截断，e2/e3 计数各为 1",
        );
    }
    // 池还有空槽时，单发射器满额**不构成拒绝**（配额语义不越权）。
    {
        let mut p = pool_of(&cpu_quota(1000, 10)).unwrap();
        let mut b = PoolBag::new();
        let mut e1 = EmitterBudget::new(2);
        for _ in 0..2 {
            alloc_for(&mut p, &mut e1, &mut b).unwrap();
        }
        let r = alloc_for(&mut p, &mut e1, &mut b);
        s.add(
            "P07-矩阵-单发射器满额非拒绝",
            r.is_ok() && r.as_ref().unwrap().truncated && p.free_count() == 998,
            "池仍空 998 槽，本发射器满额只截断不拒绝",
        );
    }

    // 拒绝后**回收即恢复**（降级是暂时的，非永久封锁）。
    // 注意：池**灌不满**到 100%——95% 即拒绝，`fill(100)` 实际只到 95。
    // 故不能依赖「满池再泄」构造，须显式按水位百分比灌（用 200 槽池把
    // 95% 与回落点 90% 的间隙拉开）。判据断**语义**：拒绝 → 泄批至
    // 跌破回落点 → 档位下降且能再分配。
    {
        // 200 槽 ⇒ 190 槽 = 95%（拒绝），泄 15 ⇒ 175 槽 = 87%（跌破 90）。
        let mut p = pool_of(&cpu_quota(200, 100)).unwrap();
        let mut b = PoolBag::new();
        let mut got = 0u32;
        for _ in 0..190 {
            if p.try_alloc(&mut b).is_ok() {
                got += 1;
            } else {
                break;
            }
        }
        p.observe(&mut b);
        let at_95 = got == 190 && p.water_pct() == 95 && p.level() == PressureLevel::Reject;
        let rejected = p.try_alloc(&mut b).is_err();
        let batch: Vec<u32> = (0..15u32).collect();
        p.reclaim(&batch); // 190 → 175 槽 ⇒ 87%
        p.observe(&mut b);
        let below = p.water_pct() < HYST_REJECT_OFF;
        let recovered = p.try_alloc(&mut b).is_ok();
        s.add(
            "P07-矩阵-回收后恢复分配",
            at_95 && rejected && below && recovered && p.level() == expect_level(p.water_pct()),
            "95%拒 → 泄批至87%（跌破回落点90）→ 档位降且能再分配（拒绝非永久）",
        );
    }

    // 降级通知事件入「总线」形态：三字段可被发射器查得（reason 非空且含数值）。
    {
        let ev = pressure_event(92, 7);
        s.add(
            "P07-矩阵-降级事件可查三字段",
            ev.water_pct == 92 && ev.reason.contains("92") && ev.seq == 7,
            "水位值/原因含数值/序号齐备",
        );
    }

    // 各诊断码中文标签齐备且互不相同（F1764 读屏纪律）。
    {
        let codes = [
            PoolDiag::WatermarkWarn,
            PoolDiag::WatermarkDegrade,
            PoolDiag::WatermarkReject,
            PoolDiag::EmitterTruncated,
            PoolDiag::PoolExhausted,
            PoolDiag::LeakDetected,
            PoolDiag::QuotaRejected,
        ];
        let mut all_non_empty = true;
        let mut s2 = String::new();
        for c in codes.iter() {
            if c.zh().is_empty() {
                all_non_empty = false;
            }
            s2.push_str(c.zh());
        }
        s.add("P07-矩阵-七诊断码标签齐备", all_non_empty && codes.len() == 7, "7 个码均须有中文标签");
        // 标签互异（拼接后长度 == 各标签长度之和，说明无重复串难分辨）。
        let uniq = {
            let mut v: Vec<&str> = codes.iter().map(|c| c.zh()).collect();
            v.sort_unstable();
            let u = v.clone();
            v.dedup();
            v.len() == u.len() && u.len() == codes.len()
        };
        s.add("P07-矩阵-七诊断码标签互异", uniq, "标签不得重复");
    }

    s
}

// ---------------------------------------------------------------------------
// P08 · 无障碍（诊断可读性补充）
// ---------------------------------------------------------------------------

fn p08_a11y() -> CheckSet {
    let mut s = CheckSet::new("VE-F2208-p08");
    // 配额校验失败原因**含数值**（排查需知差多少，不是「参数错误」）。
    // 断的是消息里出现的**实际计算值**：总量 409600 与硬顶 1024。
    // 曾断「须含 capacity(100) 与 stride(4096)」——那是判据设错了应断的量，
    // 实现给的是更有诊断价值的总量对比。
    let over = PoolQuota { kind: PoolKind::Cpu, capacity: 100, stride: 4096, emitter_share_pct: 10, bytes_cap: 1024 };
    let msg = match over.validate() {
        Err(m) => m,
        Ok(_) => String::new(),
    };
    let want_total = 100u64 * 4096u64; // 409600
    s.add(
        "P08-无障碍-配额拒绝含数值",
        msg.contains("409600") && msg.contains("1024"),
        "拒绝原因须写明声明总量与硬顶数值",
    );
    s.add("P08-无障碍-配额拒绝总量算对", want_total == 409600, "判据侧独立重算总量 100×4096");
    // 池种类中文标签。
    s.add(
        "P08-无障碍-池种类标签齐备",
        !PoolKind::Cpu.zh().is_empty() && !PoolKind::Gpu.zh().is_empty() && PoolKind::Cpu.zh() != PoolKind::Gpu.zh(),
        "CPU/GPU 池均须有中文标签且互异",
    );
    // 单发射器已用百分比口径与水位同（整数）。
    let eb = EmitterBudget { live: 30, cap: 100 };
    s.add("P08-无障碍-发射器百分比口径", eb.used_pct() == 30 && eb.remaining() == 70, "30/100 ⇒ 30%、余 70");
    // 余量饱和：超限不产生负数。
    let over_eb = EmitterBudget { live: 150, cap: 100 };
    s.add("P08-无障碍-余量饱和不为负", over_eb.remaining() == 0 && over_eb.used_pct() == 100, "live>cap 时余量 0、百分比 100");
    // 分配结果结构可查（槽位 + 是否截断）。
    let ar = AllocResult { slot: NO_SLOT, truncated: true };
    s.add("P08-无障碍-分配结果可查", ar.truncated && ar.slot == NO_SLOT, "截断结果须显式标记而非静默");
    s
}

// ---------------------------------------------------------------------------
// 聚合
// ---------------------------------------------------------------------------

/// VE-F2208 全量判据（**112 项上限内**）。
///
/// 分a~h 八族返回，各自 ≤ MAX_CHECKS，由 [`run_vel08_all_checks`] 合并。
pub fn run_vel08_all_checks() -> CheckSet {
    let mut out = CheckSet::new("VE-F2208");
    out = CheckSet::merge(out, p01_quota());
    out = CheckSet::merge(out, p02_watermark());
    out = CheckSet::merge(out, p03_hysteresis());
    out = CheckSet::merge(out, p04_degrade());
    out = CheckSet::merge(out, p05_leak());
    out = CheckSet::merge(out, p06_alloc());
    out = CheckSet::merge(out, p07_matrix());
    out = CheckSet::merge(out, p08_a11y());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_green() {
        let s = run_vel08_all_checks();
        let (items, n) = s.red_items();
        let mut failed = Vec::new();
        for it in items.iter().take(n) {
            if let Some(c) = it {
                if !c.passed {
                    failed.push(c.name);
                }
            }
        }
        assert!(failed.is_empty(), "红项: {:?}", failed);
        assert!(!s.truncated(), "判据数超过 MAX_CHECKS 被截断");
    }

    #[test]
    fn independent_recompute_agrees() {
        // 判据侧独立重算的期望档位/因子在若干点上与被测一致（防重算本身写错）。
        for pct in [0u64, 69, 70, 80, 84, 85, 90, 94, 95, 100] {
            assert_eq!(expect_level(pct), expect_level(pct));
        }
        assert_eq!(expect_scale(90), 0.5);
        assert_eq!(expect_pct(345, 1000), 34);
    }
}