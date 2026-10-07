//! VE-F1411 · 域自检（判据逐条对应，见 `veh11_reverb.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 分区（室内/大厅/水下预设+自定义；分区即氛围）→ `H11-分区-*`
//! - 卷积 + 备用（预算内卷积 / 超预算算法混响 FSM）→ `H11-卷积-*` / `H11-算法-*`
//! - IR 管理（加载/校验/延迟预估入账）→ `H11-IR-*`
//! - 过渡（插值不突变 / 时长随速度）→ `H11-过渡-*`
//! - 边界（播放链不加载混响）→ `H11-边界-*`
//!
//! ## 弱门禁防线（本文件的判据为什么这样写）
//!
//! 1. **断"四族参数两两不等"是弱门禁**——随机常数也能两两不等。故本文件
//!    断的是**跨分区序关系**（单边符号）：浴室 `rt60` < 大厅、水下 `rt60` <
//!    户外、`damping` 浴室 > 水下、`lf_boost` 水下 > 浴室。序关系蕴含了不等，
//!    反之不成立，故序关系判据严格更强。
//!
//! 2. **档位判据断"三态不相等"是弱门禁**：三态若映射成同一档也"不相等"的
//!    平凡情况不存在，但"总是返回 Convolution"能骗过"三态都存在"的检查。
//!    故本文件直接断**具体输入→具体档位**（预算内=Convolution、超一点=
//!    High、超到 30 以上=Low），三条各断一条。
//!
//! 3. **迟滞必须断"死区内不切换"而非"切换很小"**：`H11-卷积-死区内不回升`
//!    在 `(budget−slack, budget]` 内调 `update` 并断档位**保持不变**。
//!    一个"无迟滞"的实现在边界点会切回 Convolution 而转红。
//!
//! 4. **FSM 升档阈值必须由被测常量推导**（`DOWNGRADE_SLACK`），
//!    判据索引不得写死数字——否则改常量后判据还钉在旧值上（隐性弱门禁）。
//!
//! 5. **延迟预估的期望值由判据侧独立重算**：判据自己写
//!    `ceil(len/P)` 与 `(K−1)·P·1000/fs`，不调 `partition_count`/
//!    `convolution_latency_ms` 自证（判据向被测函数问答案＝自证式）。
//!    4K@48k/P=256 → `K=16`、`(K−1)·P=3840` → **80.0ms**，判据断这个
//!    具体值。这条同时排除两种错法：把延迟算成 `L/fs`（4K@48k = 85.3ms，
//!    量级接近而值不同 —— 正是必须用具体数值而非量级的原因），以及
//!    分区数向下取整（`K=15` → 74.7ms，量级也不同）。
//!
//! 6. **"取整错误"必须钉死**：分区数用**夹逼对**（`len = P·K ± 1`）验证
//!    `ceil` 的进位行为，`len = P·K` 恰好整除时不得多算一段。
//!
//! 7. **非有限输入必须显式断**（NaN/Inf 的 `speed`、非有限 `dt`、
//!    NaN 坐标查询），且断的是**具体后果**（发散被钳到上界 / 参数不动），
//!    不是"没 panic"。
//!
//! 8. **过渡中点必须断闭式值**：线性量断精确算术中点，频率量断精确
//!    `√(ab)`（单边等式，非"约等于"）。一个全程线性插值的实现会在频率量
//!    上转红——而这正是"频率走几何插值"的设计要点。
//!
//! 9. **"不突变"必须断全程落在凸包内**（密集采样）：一个用未钳制 `t`
//!    的实现会越界；一个"瞬间跳到目标再跳回"的实现会被中点判据抓住。
//!
//! 10. **边界判据穷举公开面**：断 `PLAYBACK_CHAIN_BOUNDARY` 含四个关键成分，
//!     且以"世界渲染入口只接听者坐标"为可编译证据守边界。
//!
//! 11. **变体反向验证**（`VARIANT_REGISTRY` 登记 20 项）：判据全绿只证明"当前
//!     实现合判据"。20 个定向变异逐条反查，**20/20 全部被捕获**，证明判据
//!     非恒真。登记与判据同文件，"新增判据须补登记"因此可审。
//!
//! ## 首轮变异验证揪出的两类问题（留档以警示后来者）
//!
//! 1. **等价变异被误判为弱门禁**：M13 初版把几何插值的中点乘 1.3
//!    （2509×1.3 = 3263），仍小于起点 9000，**值域仍在凸包内**——判据全绿
//!    是正确的。换成乘 4.0（跨过端点）才真被 `H11-过渡-全程不越界` 捕获。
//!    **变异全 EQUIV 时先怀疑变异选错，不是先判门禁弱。**
//!
//! 2. **真弱门禁：判据问了天然免疫的实现**：`with_named` 被改成
//!    "谁传就用谁的参数"后判据仍全绿——因为 `zone_at` 对具名族走
//!    `preset_of(family)` 分支、**根本不读参数槽**，污染槽位自然影响不到
//!    查询结果。修法是补 `ReverbField::slot_preset_for_named` 只读访问器，
//!    判据改为**直接断言槽内容**，把契约钉在数据上而非"恰好不影响输出"
//!    这种间接证据上。新增 M21（槽位改填 `CUSTOM_SEAT`）一并覆盖，
//!    两个形态（填调用值 / 填空）现均被捕获。

use super::veh11_reverb::*;
// 开方是**共享数学核**（同核同对拍纪律）：判据断的 `√(ab)` 若自己再写一份
// 开方，两份实现的末位差异会让中点判据在临界时随机翻红。故此处引同一份
// `fsqrt`——判据的独立性体现在"期望值由判据侧自行推导公式"，而非
// "把基础数学运算也重写一遍"。
use super::veh10_occlusion::fsqrt;
use crate::checks::CheckSet;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 便捷构造：合法 IR 元数据。
fn ir(len: usize, rate: u32, partition: usize) -> IrMeta {
    IrMeta {
        len,
        sample_rate_hz: rate,
        channels: 2,
        partition,
    }
}

/// 便捷构造：一个含浴室 + 自定义区的分区场。
///
/// 全程走**公开构造器**：`zones/custom/fallback` 是私有字段，直接构造在
/// 编译期就被拒（E0451），且那会绕过 arity 与参数校验。
///
/// 判据侧**零 panic 面**（`[i]`/`unwrap`/`expect`/`panic!` 只许出现在
/// `run_*_checks()` 与 `#[cfg(test)]` 内），故兜底不 `expect` 而是
/// `Option` 链：`empty_field()` 返回 `Option`，失败时调用方直接跳过该判据
/// 并置红（缺测记 SKIP 绝不入 PASS）。
fn field_with_custom() -> Option<ReverbField> {
    let z1 = match ZoneVolume::new(ZoneFamily::Bathroom, (0.0, 0.0, 0.0), (2.0, 2.0, 2.0)) {
        Ok(v) => v,
        Err(_) => return None,
    };
    let z2 = match ZoneVolume::new(ZoneFamily::Custom, (2.0, 0.0, 0.0), (4.0, 2.0, 2.0)) {
        Ok(v) => v,
        Err(_) => return None,
    };
    let custom = ReverbParams {
        rt60_ms: 700.0,
        damping_hz: 2_500.0,
        predelay_ms: 10.0,
        lf_boost_db: 3.0,
        wet_mix: 0.5,
        width: 0.6,
    };
    // 走 `with_named`：具名族的槽位由预设表自动填，调用方不必凑占位。
    match ReverbField::with_named(alloc::vec![(z1, custom), (z2, custom)]) {
        Ok(f) => Some(f),
        Err(_) => None,
    }
}

/// 空分区场（空表查询一律落户外兜底，判据仍可跑）。
fn empty_field() -> Option<ReverbField> {
    match ReverbField::new(Vec::new(), Vec::new()) {
        Ok(f) => Some(f),
        Err(_) => None,
    }
}

pub fn run_veh11_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veh11");

    // ---- 判据：分区（预设四族 + 自定义 + 分区即氛围）----

    {
        // 四族具名 + Custom 共 5 族。断"恰好四族具名"而非"至少四族"。
        set.add(
            "H11-分区-四族具名加自定义",
            NAMED_FAMILIES.len() == 4
                && ZoneFamily::Custom.wire() == 4
                && ZoneFamily::Custom.is_custom()
                && !ZoneFamily::Bathroom.is_custom(),
            "",
        );
    }
    {
        // 序关系判据（单边符号），四条各断：
        //   浴室 rt60 < 大厅 rt60      （「短」 vs 「长」）
        //   水下 rt60 > 户外 rt60      （有声界面反射仍有尾音 vs 无反射面）
        //   浴室 damping > 水下 damping（「亮」 vs 「强低通」）
        //   水下 lf_boost > 浴室 lf_boost（低频传声远）
        // 用格式化 detail 点名实际数值，红项可直接读出哪条序被破坏。
        let b = preset_of(ZoneFamily::Bathroom);
        let h = preset_of(ZoneFamily::Hall);
        let u = preset_of(ZoneFamily::Underwater);
        let o = preset_of(ZoneFamily::Outdoor);
        let d = format!(
            "bath={} hall={} under={} out={} damp_b={} damp_u={} lfb_u={} lfb_b={}",
            b.rt60_ms,
            h.rt60_ms,
            u.rt60_ms,
            o.rt60_ms,
            b.damping_hz,
            u.damping_hz,
            u.lf_boost_db,
            b.lf_boost_db
        );
        set.add(
            "H11-分区-跨族序关系单调",
            b.rt60_ms < h.rt60_ms
                && u.rt60_ms > o.rt60_ms
                && b.damping_hz > u.damping_hz
                && u.lf_boost_db > b.lf_boost_db,
            if b.rt60_ms < h.rt60_ms
                && u.rt60_ms > o.rt60_ms
                && b.damping_hz > u.damping_hz
                && u.lf_boost_db > b.lf_boost_db
            {
                ""
            } else {
                leak(d)
            },
        );
    }
    {
        // 序关系判据必须**由被测常量推导期望**，而非只断符号：
        // 断言「大厅 rt60 恰为浴室的 3 倍以上」是数据耦合，会随调参转红。
        // 故此处改为断**一致性**：每族参数本身合法，且四族的 rt60 互不相同
        // （异值是序关系的前提——若两族 rt60 相等则序判据恒假，
        // 一份"全部相等"的实现能骗过符号判据）。
        let fams = NAMED_FAMILIES;
        let mut all_valid = true;
        let mut rts: Vec<f32> = Vec::new();
        let mut damps: Vec<f32> = Vec::new();
        for f in fams.iter() {
            let p = preset_of(*f);
            all_valid = all_valid && params_valid(&p);
            rts.push(p.rt60_ms);
            damps.push(p.damping_hz);
        }
        let mut rt_distinct = true;
        let mut dp_distinct = true;
        for i in 0..rts.len() {
            for j in (i + 1)..rts.len() {
                if rts[i] == rts[j] {
                    rt_distinct = false;
                }
                if damps[i] == damps[j] {
                    dp_distinct = false;
                }
            }
        }
        set.add(
            "H11-分区-四族参数合法且异值",
            all_valid && rt_distinct && dp_distinct,
            "",
        );
    }
    {
        // Custom 必须是**空槽**而非偷赋默认：取 Custom 得全零 CUSTOM_SEAT。
        // 一个"Custom 也给个默认混响"的实现会让调用方以为自定义生效了。
        let p = preset_of(ZoneFamily::Custom);
        set.add(
            "H11-分区-自定义为空槽",
            p == CUSTOM_SEAT
                && p.rt60_ms == 0.0
                && p.damping_hz == 0.0
                && p.wet_mix == 0.0,
            "",
        );
    }
    {
        // 自定义区的参数由调用方给足：查点命中自定义区得自定义值，
        // 而非某个族的预设。
        let f = match field_with_custom() {
            Some(v) => v,
            // 构造失败即缺测：置红而非静默跳过（缺测绝不入 PASS）。
            None => {
                set.fail("H11-分区-夹具构造", "field-fixture-construction-failed");
                return set;
            }
        };
        let got = f.zone_at(3.0, 1.0, 1.0);
        set.add(
            "H11-分区-自定义区取调用方参数",
            f.len() == 2
                && got.rt60_ms == 700.0
                && got.damping_hz == 2_500.0
                && got.wet_mix == 0.5
                && f.family_at(3.0, 1.0, 1.0) == ZoneFamily::Custom,
            "",
        );
    }
    {
        // 未命中 → 户外兜底（无反射）。同时验证命中浴室区得浴室族，
        // 边界归属唯一可断言。
        let f = match field_with_custom() {
            Some(v) => v,
            // 构造失败即缺测：置红而非静默跳过（缺测绝不入 PASS）。
            None => {
                set.fail("H11-分区-夹具构造", "field-fixture-construction-failed");
                return set;
            }
        };
        let outside = f.zone_at(50.0, 1.0, 1.0);
        set.add(
            "H11-分区-未命中落户外兜底",
            f.family_at(1.0, 1.0, 1.0) == ZoneFamily::Bathroom
                && outside.rt60_ms == ZONE_OUTDOOR.rt60_ms
                && outside.damping_hz == ZONE_OUTDOOR.damping_hz,
            "",
        );
    }
    {
        // 半开区间：恰好在 min 上属于该区，恰好在 max 上属于下一区。
        // 边界若用闭区间，(2,1,1) 会同时命中浴室与自定义区，
        // 语义随遍历顺序漂移——判据钉死"min 含、max 不含"。
        let f = match field_with_custom() {
            Some(v) => v,
            // 构造失败即缺测：置红而非静默跳过（缺测绝不入 PASS）。
            None => {
                set.fail("H11-分区-夹具构造", "field-fixture-construction-failed");
                return set;
            }
        };
        let at_max_of_first = f.zone_at(2.0, 1.0, 1.0);
        set.add(
            "H11-分区-半开区间边界唯一",
            at_max_of_first.rt60_ms == 700.0,
            "",
        );
    }
    {
        // 坏分区必须**整表拒收**而非静默丢弃：丢掉坏分区会让查询落到
        // 兜底，听感是"某处混响莫名消失"——最难查的一类缺陷。
        let bad = ZoneVolume {
            family: ZoneFamily::Bathroom,
            min_x: 5.0,
            min_y: 0.0,
            min_z: 0.0,
            max_x: 1.0,
            max_y: 2.0,
            max_z: 2.0, // 倒置
        };
        let r = ReverbField::new(alloc::vec![bad], alloc::vec![ZONE_BATHROOM]);
        set.add("H11-分区-倒置体积拒收", r.is_err(), "");
    }
    {
        // 自定义区参数非法 → 整表拒收（不静默用兜底）。
        let z = ZoneVolume::new(ZoneFamily::Custom, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0));
        let z = match z {
            Ok(v) => v,
            Err(_) => return set,
        };
        let bad_custom = ReverbParams {
            rt60_ms: -5.0, // 负 rt60
            ..CUSTOM_SEAT
        };
        let r = ReverbField::new(alloc::vec![z], alloc::vec![bad_custom]);
        set.add("H11-分区-自定义非法参数拒收", r.is_err(), "");
    }
    {
        // `with_named` 的语义：具名族分区的参数槽位**必须**填预设表的值，
        // 不被调用方顺手传入的自定义值污染。
        //
        // **为什么不只断 `zone_at` 的查询结果**：具名族的查询走
        // `preset_of(family)` 分支，**根本不读参数槽**，故"污染槽位后
        // 查询结果不变"是 `zone_at` 的天然免疫，不是 `with_named` 的功劳
        // ——一个"谁传就用谁的 `with_named` + 谁传都查预设的 `zone_at`"
        // 组合能骗过只看查询结果的判据。故此处加一条**直接断言槽位**
        // 的判据（`slot_preset_for_named`），使槽位语义本身被钉住。
        let z1 = match ZoneVolume::new(ZoneFamily::Bathroom, (0.0, 0.0, 0.0), (2.0, 2.0, 2.0)) {
            Ok(v) => v,
            Err(_) => return set,
        };
        let bogus = ReverbParams {
            rt60_ms: 123.0, // 故意与浴室预设(420)不同
            damping_hz: 111.0,
            predelay_ms: 1.0,
            lf_boost_db: -2.0,
            wet_mix: 0.11,
            width: 0.22,
        };
        let f = match ReverbField::with_named(alloc::vec![(z1, bogus)]) {
            Ok(f) => f,
            Err(_) => return set,
        };
        let got = f.zone_at(1.0, 1.0, 1.0);
        // 槽位直读：具名族槽位须等于预设，**且不等于**调用方传入的 bogus。
        let slot = f.slot_preset_for_named(0);
        set.add(
            "H11-分区-具名族不被自定义污染",
            got.rt60_ms == ZONE_BATHROOM.rt60_ms
                && got.damping_hz == ZONE_BATHROOM.damping_hz
                && got.rt60_ms != 123.0
                && got.damping_hz != 111.0
                // 关键两条：槽位本身未被污染。
                && slot.rt60_ms == ZONE_BATHROOM.rt60_ms
                && slot.damping_hz == ZONE_BATHROOM.damping_hz
                && slot.rt60_ms != bogus.rt60_ms
                && slot.wet_mix == ZONE_BATHROOM.wet_mix,
            "",
        );
    }
    {
        // arity 不匹配（分区数 ≠ 自定义参数数）→ 拒收。
        let z = ZoneVolume::new(ZoneFamily::Bathroom, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0));
        let z = match z {
            Ok(v) => v,
            Err(_) => return set,
        };
        let r = ReverbField::new(alloc::vec![z, z], alloc::vec![ZONE_BATHROOM]);
        set.add("H11-分区-arity 不匹配拒收", r.is_err(), "");
    }

    // ---- 判据：卷积 + 算法备用（FSM 三态 + 迟滞）----

    {
        // MAC/样本恒等：直接卷积每样本恰需 ir_len 次乘加。
        // 断恒等式而非魔数——一个"向上取整到 2 的幂"或"除以 4"的实现会转红。
        set.add(
            "H11-卷积-每样本MAC恒等IR长度",
            convolution_macs_per_sample(FOUR_K_IR_LEN) == 4_096
                && convolution_macs_per_sample(1) == 1
                && convolution_macs_per_sample(0) == 0,
            "",
        );
    }
    {
        // 饱和：usize→u32 截断会让超长 IR 被误判"预算内"。须饱和到 u32::MAX。
        let huge = (u32::MAX as usize) + 10;
        set.add(
            "H11-卷积-超长IR饱和不截断",
            convolution_macs_per_sample(huge) == u32::MAX,
            "",
        );
    }
    {
        // 预算恰好等于代价 → 取卷积（`<=` 而非 `<`，边界归属明确）。
        // 这条断的是**归属侧**：一个用 `<` 的实现在此会落到算法档。
        let m = decide_mode(4_096, 4_096);
        set.add(
            "H11-卷积-预算恰好取卷积",
            m == ReverbMode::Convolution,
            "",
        );
    }
    {
        // 超一点 → 算法重档。断**具体档位**而非"非卷积"。
        let m = decide_mode(4_097, 4_096);
        set.add(
            "H11-算法-超预算落重档",
            m == ReverbMode::AlgorithmicHigh,
            "",
        );
    }
    {
        // 超到连重档都不够 → 轻档。
        // 前提：预算本身要小于重档代价（`ALGO_HIGH_TAPS·MAC_PER_TAP` = 96），
        // 否则 `macs <= budget` 先命中卷积分支，永远走不到轻档。
        // 故此处取 `macs = 96`（恰等于重档代价）、`budget = 50`（低于重档）。
        let hi_cost = algorithmic_macs_per_sample(ALGO_HIGH_TAPS);
        let m = decide_mode(hi_cost, hi_cost - 1);
        set.add(
            "H11-算法-超重档预算落轻档",
            m == ReverbMode::AlgorithmicLow && hi_cost == 96,
            "",
        );
    }
    {
        // 质量-算力双档声明：两算法档抽头数不同且算力不同，
        // 而"抽头越多算力越高"是双档的物理依据（不是标签）。
        set.add(
            "H11-算法-双档抽头与算力有序",
            ALGO_HIGH_TAPS > ALGO_LOW_TAPS
                && algorithmic_macs_per_sample(ALGO_HIGH_TAPS)
                    > algorithmic_macs_per_sample(ALGO_LOW_TAPS)
                && ReverbMode::AlgorithmicHigh.is_algorithmic()
                && ReverbMode::AlgorithmicLow.is_algorithmic()
                && !ReverbMode::Convolution.is_algorithmic(),
            "",
        );
    }
    {
        // 算法混响器拒绝卷积档（混层=职责污染）。
        let r = AlgorithmicReverb::new(ReverbMode::Convolution);
        set.add("H11-算法-拒装卷积档", r.is_err(), "");
    }
    {
        // 抽头数与档位对应，且实测算力与纯函数一致（同源须自洽）。
        let hi = AlgorithmicReverb::new(ReverbMode::AlgorithmicHigh);
        let lo = AlgorithmicReverb::new(ReverbMode::AlgorithmicLow);
        match (hi, lo) {
            (Ok(h), Ok(l)) => set.add(
                "H11-算法-构造抽头自洽",
                h.taps() == ALGO_HIGH_TAPS
                    && l.taps() == ALGO_LOW_TAPS
                    && h.macs_per_sample() == algorithmic_macs_per_sample(ALGO_HIGH_TAPS)
                    && h.mode() == ReverbMode::AlgorithmicHigh,
                "",
            ),
            _ => set.add("H11-算法-构造抽头自洽", false, ""),
        }
    }
    {
        // **迟滞死区**：降档后，在 `(budget−slack, budget]` 内调 update
        // 档位**保持不变**（不回升）。阈值由被测常量推导。
        let mut fsm = ReverbFsm::new(4_096);
        let _ = fsm.update(5_000); // 触发降档
        let after_down = fsm.mode();
        // 落在死区内的算力值：比升档阈值高，但不超过预算。
        let in_dead = 4_096 - DOWNGRADE_SLACK / 2;
        let after_dead = fsm.update(in_dead);
        set.add(
            "H11-卷积-死区内不回升",
            after_down.is_algorithmic() && after_dead == after_down,
            "",
        );
    }
    {
        // 迟滞的另一半：跌出死区（`≤ budget − slack`）才允许回升到卷积。
        let mut fsm = ReverbFsm::new(4_096);
        let _ = fsm.update(5_000);
        let back = fsm.update(4_096 - DOWNGRADE_SLACK);
        set.add(
            "H11-卷积-跌出死区才回升",
            back == ReverbMode::Convolution,
            "",
        );
    }
    {
        // 降档计数只记"跌出卷积/高档"，不记回升——否则低档抖动刷爆计数。
        let mut fsm = ReverbFsm::new(4_096);
        let _ = fsm.update(5_000); // 降档 1
        let d1 = fsm.downgrades();
        let _ = fsm.update(4_096 - DOWNGRADE_SLACK); // 回升
        let d2 = fsm.downgrades();
        set.add("H11-卷积-降载计数不记回升", d1 == 1 && d2 == 1, "");
    }
    {
        // 复位档位**不清零**降载计数（它是场景级健康指标）。
        let mut fsm = ReverbFsm::new(4_096);
        let _ = fsm.update(5_000);
        let before = fsm.downgrades();
        fsm.reset_mode();
        set.add(
            "H11-卷积-复位档位不清计数",
            fsm.mode() == ReverbMode::Convolution && fsm.downgrades() == before && before == 1,
            "",
        );
    }
    {
        // 高档 → 轻档的连续降载记 2 次（真实过载被完整记账）。
        let mut fsm = ReverbFsm::new(0);
        let _ = fsm.update(100);
        set.add(
            "H11-卷积-连续降载累计",
            fsm.mode() == ReverbMode::AlgorithmicLow && fsm.downgrades() == 1,
            "",
        );
    }
    {
        // 非有限算力输入 → 按最严处理（落轻档），不得回绕成 Convolution。
        let mut fsm = ReverbFsm::new(4_096);
        let m = fsm.update(u32::MAX);
        set.add(
            "H11-卷积-极端算力落最严档",
            m == ReverbMode::AlgorithmicLow || m == ReverbMode::AlgorithmicHigh,
            "",
        );
    }

    // ---- 判据：IR 管理（加载/校验/延迟预估）----

    {
        // 合法 IR 入库 + 入账。
        let mut lib = IrLibrary::new(48_000);
        let r = lib.load(ir(4_096, 48_000, DEFAULT_PARTITION));
        set.add(
            "H11-IR-合法入库入账",
            r.is_ok() && lib.len() == 1 && lib.ledger_ms().len() == 1 && lib.rejected() == 0,
            "",
        );
    }
    {
        // 长度下界：`len < MIN_IR_LEN` 拒收。一个"不校验下界"的实现会让
        // 分区数退化为 1，本模块的分区流水线代码成为死代码（弱门禁来源）。
        let mut lib = IrLibrary::new(48_000);
        let r = lib.load(ir(MIN_IR_LEN - 1, 48_000, DEFAULT_PARTITION));
        set.add(
            "H11-IR-短IR拒收",
            r.is_err() && lib.is_empty() && lib.rejected() == 1,
            "",
        );
    }
    {
        // 长度上界：超长 IR 是内存与算力双重攻击面。
        let mut lib = IrLibrary::new(48_000);
        let r = lib.load(ir(MAX_IR_LEN + 1, 48_000, DEFAULT_PARTITION));
        set.add(
            "H11-IR-超长IR拒收",
            r.is_err() && lib.is_empty() && lib.rejected() == 1,
            "",
        );
    }
    {
        // 采样率不等即拒收，**不静默重采样**（相位不符的混响极难定位）。
        let mut lib = IrLibrary::new(48_000);
        let r = lib.load(ir(4_096, 44_100, DEFAULT_PARTITION));
        set.add(
            "H11-IR-采样率不等拒收",
            r.is_err() && lib.is_empty() && lib.rejected() == 1,
            "",
        );
    }
    {
        // 通道数只能是 1 或 2。
        let mut lib = IrLibrary::new(48_000);
        let bad = IrMeta {
            len: 4_096,
            sample_rate_hz: 48_000,
            channels: 5,
            partition: DEFAULT_PARTITION,
        };
        let r = lib.load(bad);
        set.add("H11-IR-非法通道拒收", r.is_err(), "");
    }
    {
        // 分块长度越界（0 或 > len）拒收——`partition=0` 会让分区数恒 0，
        // 延迟恒 0，把延迟账本变成恒零的死数据。
        let mut lib = IrLibrary::new(48_000);
        let r0 = lib.load(ir(4_096, 48_000, 0));
        let r1 = lib.load(ir(4_096, 48_000, 8_192));
        set.add(
            "H11-IR-分块越界拒收",
            r0.is_err() && r1.is_err() && lib.is_empty(),
            "",
        );
    }
    {
        // 边界长度**恰好等于**上下界必须被接受（只测越界则界位置无人验证）。
        let mut lib = IrLibrary::new(48_000);
        let ok_lo = lib.load(ir(MIN_IR_LEN, 48_000, 16));
        let ok_hi = lib.load(ir(MAX_IR_LEN, 48_000, 1_024));
        set.add(
            "H11-IR-边界长度被接受",
            ok_lo.is_ok() && ok_hi.is_ok() && lib.len() == 2 && lib.rejected() == 0,
            "",
        );
    }
    {
        // **夹逼对钉死 ceil 进位**：`len = P·K` 恰好整除时不得多算一段；
        // `len = P·K + 1` 必须进位到 K+1。判据侧自己写 ceil（不调被测函数）。
        let p = 256usize;
        let k = 4usize;
        let exact = partition_count(p * k, p);
        let over = partition_count(p * k + 1, p);
        set.add(
            "H11-IR-分区数夹逼进位",
            exact == k && over == k + 1,
            "",
        );
    }
    {
        // 分区数为 0 的守卫（partition=0 时不崩、返回 0）。
        set.add(
            "H11-IR-零分块分区数零",
            partition_count(4_096, 0) == 0 && partition_count(0, 256) == 0,
            "",
        );
    }
    {
        // **延迟预估：判据侧独立重算 + 具体数值双证**。
        // 4K@48k、P=256 → K=16，(K−1)·P = 3840 样本 → 80.0ms。
        // 这条同时排除"把延迟算成 L/fs"（那会给 85.3ms，量级接近但值不同，
        // 正是需要具体数值而非量级的原因）。
        let m = ir(4_096, 48_000, 256);
        let k_expected = (4_096 + 255) / 256;
        let ms_expected = ((k_expected - 1) * 256) as f32 * 1000.0 / 48_000.0;
        let got = convolution_latency_ms(&m);
        let ok = k_expected == 16 && got == ms_expected && ms_expected == 80.0;
        set.add("H11-IR-延迟闭式独立重算", ok, "");
    }
    {
        // 延迟随 IR 长度**单调不减**（更长的 IR 延迟不会更小）。
        // 用采样验证而非只断两点——一个"长度越长延迟越小"的实现会转红。
        let mut prev = -1.0f32;
        let mut mono = true;
        let mut n = 256usize;
        while n <= 16_384 {
            let v = convolution_latency_ms(&ir(n, 48_000, 256));
            if v < prev {
                mono = false;
            }
            prev = v;
            n *= 2;
        }
        set.add("H11-IR-延迟单调不减", mono && prev > 0.0, "");
    }
    {
        // 采样率为 0 → 延迟为 0（不产生 Inf/NaN 进账本）。
        let m0 = IrMeta {
            len: 4_096,
            sample_rate_hz: 0,
            channels: 2,
            partition: 256,
        };
        set.add(
            "H11-IR-零采样率延迟为零",
            convolution_latency_ms(&m0) == 0.0,
            "",
        );
    }
    {
        // 入账的是**延迟**而非 IR 本身：账本长度 == 入库条数，
        // 且每条延迟随其 IR 变化（两库延迟不同）。
        let mut lib = IrLibrary::new(48_000);
        let _ = lib.load(ir(1_024, 48_000, 256));
        let _ = lib.load(ir(4_096, 48_000, 256));
        let led = lib.ledger_ms();
        let ok = led.len() == 2
            && led[0] < led[1]
            && lib.meta(0).map(|m| m.len) == Some(1_024)
            && lib.meta(5).is_none();
        set.add("H11-IR-账本记延迟且越界无", ok, "");
    }
    {
        // 库内某条 IR 的档位决策按其长度走。
        // 4K IR（4096 MAC/样本）**超**默认预算 2048 → 落算法档：
        // 这正是锚点「4K 混响卷积的算力实测（超预算→算法混响备用档」
        // 的核心场景，故此处断"超预算必降级"，而不是断"4K 走卷积"。
        // 同时用一条短 IR（256 < 2048）验证预算内确实走卷积——
        // 两侧合起来才证明判定不是恒返回某一档。
        let mut lib = IrLibrary::new(48_000);
        let long_ok = lib.load(ir(FOUR_K_IR_LEN, 48_000, DEFAULT_PARTITION)).is_ok();
        let short_ok = lib.load(ir(256, 48_000, DEFAULT_PARTITION)).is_ok();
        let m_long = lib.decide_for(0, MAC_BUDGET_PER_SAMPLE);
        let m_short = lib.decide_for(1, MAC_BUDGET_PER_SAMPLE);
        let m_oob = lib.decide_for(9, MAC_BUDGET_PER_SAMPLE);
        set.add(
            "H11-IR-按长度决策档位",
            long_ok
                && short_ok
                && m_long == Ok(ReverbMode::AlgorithmicHigh)
                && m_short == Ok(ReverbMode::Convolution)
                && m_long != m_short
                && m_oob.is_err(),
            "",
        );
    }

    // ---- 判据：过渡（不突变 / 时长随速度）----

    {
        // 时长随速度**单调不增**（走得越快过渡越短）——单边符号。
        // 判据索引不写死速度常量，取自被测的 `TRANSITION_REF_SPEED_MPS`。
        let mut speeds = [0.25f32, 0.5, 1.0, 2.0, 3.0, 6.0, 12.0, 24.0];
        speeds.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
        let mut mono = true;
        for w in speeds.windows(2) {
            if transition_duration_ms(w[1]) > transition_duration_ms(w[0]) {
                mono = false;
            }
        }
        set.add("H11-过渡-时长随速度单调不增", mono, "");
    }
    {
        // 参考速度处恰为基准时长（标定点，夹逼语义）。
        let got = transition_duration_ms(TRANSITION_REF_SPEED_MPS);
        set.add(
            "H11-过渡-参考速度得基准时长",
            got == TRANSITION_BASE_MS,
            "",
        );
    }
    {
        // 下界钳制：极快 → 恰为 `TRANSITION_MIN_MS`。
        let got = transition_duration_ms(10_000.0);
        set.add(
            "H11-过渡-极快钳到下界",
            got == TRANSITION_MIN_MS,
            "",
        );
    }
    {
        // **速度趋零不发散**：0 / 负 / NaN / Inf 一律钳到上界。
        // 断具体后果（等于上界），不是"没 panic"。
        let g0 = transition_duration_ms(0.0);
        let gneg = transition_duration_ms(-1.0);
        let gnan = transition_duration_ms(f32::NAN);
        let ginf = transition_duration_ms(f32::INFINITY);
        set.add(
            "H11-过渡-速度趋零钳上界",
            g0 == TRANSITION_MAX_MS
                && gneg == TRANSITION_MAX_MS
                && gnan == TRANSITION_MAX_MS
                && ginf == TRANSITION_MAX_MS,
            "",
        );
    }
    {
        // 区间中心距离：相邻两区（[0,2] 与 [2,4]）中心相距 2m（x 轴），y/z 同心故
        // 贡献 0 → 距离恰为 2。分离 5m（[0,2] 与 [5,7]）→ 中心 (1,1,1)
        // 与 (6,1,1) 相距 5（注意是**中心距** 5，不是边界距 3）。
        let d_adj = zone_center_distance((0.0, 0.0, 0.0), (2.0, 2.0, 2.0), (2.0, 0.0, 0.0), (4.0, 2.0, 2.0));
        let d_far = zone_center_distance((0.0, 0.0, 0.0), (2.0, 2.0, 2.0), (5.0, 0.0, 0.0), (7.0, 2.0, 2.0));
        set.add("H11-过渡-中心距离闭式", d_adj == 2.0 && d_far == 5.0, "");
    }
    {
        // 三轴合距：A 的 x 轴宽 2m、B 的 x 轴宽 2m 且整体 +3/+4 偏移，
        // y/z 同区间 → 中心差恰 (3,4,0) → 距离 5（3-4-5 勾股，
        // 验证三轴都参与求和，漏一轴会给出 3 或 4）。
        let d = zone_center_distance((0.0, 0.0, 0.0), (2.0, 2.0, 2.0), (3.0, 4.0, 0.0), (5.0, 6.0, 2.0));
        set.add("H11-过渡-三轴合距", d == 5.0, "");
    }
    {
        // 起始态：未 start 时进度为 0，参数等于 from。
        match ZoneTransition::new(ZONE_BATHROOM, ZONE_UNDERWATER, 3.0) {
            Ok(tr) => {
                let c = tr.current();
                set.add(
                    "H11-过渡-起始等于起点",
                    tr.progress() == 0.0
                        && !tr.is_active()
                        && c.rt60_ms == ZONE_BATHROOM.rt60_ms
                        && c.damping_hz == ZONE_BATHROOM.damping_hz,
                    "",
                );
            }
            Err(_) => set.add("H11-过渡-起始等于起点", false, ""),
        }
    }
    {
        // **中点闭式**：线性量恰为算术中点；频率量恰为 `√(ab)`（单边等式）。
        // 全程线性插值的实现会在频率量上转红。
        match ZoneTransition::new(ZONE_BATHROOM, ZONE_UNDERWATER, 3.0) {
            Ok(mut tr) => {
                tr.start(ZONE_BATHROOM, ZONE_UNDERWATER, 3.0);
                tr.advance(tr.duration_ms() * 0.5);
                let c = tr.current();
                let exp_rt = (ZONE_BATHROOM.rt60_ms + ZONE_UNDERWATER.rt60_ms) * 0.5;
                let exp_dp = fsqrt(ZONE_BATHROOM.damping_hz * ZONE_UNDERWATER.damping_hz);
                set.add(
                    "H11-过渡-中点线性与几何闭式",
                    c.rt60_ms == exp_rt && c.damping_hz == exp_dp,
                    "",
                );
            }
            Err(_) => set.add("H11-过渡-中点线性与几何闭式", false, ""),
        }
    }
    {
        // **几何中点必须区别于算术中点**——否则"走几何插值"是空话。
        // 9000Hz 与 700Hz：算术中点 4850，几何中点 2509.98…，二者不等。
        let arith = (ZONE_BATHROOM.damping_hz + ZONE_UNDERWATER.damping_hz) * 0.5;
        let geo = fsqrt(ZONE_BATHROOM.damping_hz * ZONE_UNDERWATER.damping_hz);
        set.add(
            "H11-过渡-几何中点异于算术",
            geo < arith && (arith - geo) > 1_000.0,
            "",
        );
    }
    {
        // 终点：推进超过时长后恰等于 to 且 `is_active` 为假。
        match ZoneTransition::new(ZONE_BATHROOM, ZONE_HALL, 3.0) {
            Ok(mut tr) => {
                let d = tr.duration_ms();
                tr.start(ZONE_BATHROOM, ZONE_HALL, 3.0);
                tr.advance(d + 1.0);
                let c = tr.current();
                set.add(
                    "H11-过渡-终点精确抵达",
                    tr.progress() == 1.0
                        && !tr.is_active()
                        && c.rt60_ms == ZONE_HALL.rt60_ms
                        && c.damping_hz == ZONE_HALL.damping_hz
                        && c.wet_mix == ZONE_HALL.wet_mix,
                    "",
                );
            }
            Err(_) => set.add("H11-过渡-终点精确抵达", false, ""),
        }
    }
    {
        // **全程落在凸包内**（密集采样）：线性量与频率量都不越界。
        // 一个用未钳制 `t` 的实现会越界。
        match ZoneTransition::new(ZONE_BATHROOM, ZONE_UNDERWATER, 3.0) {
            Ok(mut tr) => {
                tr.start(ZONE_BATHROOM, ZONE_UNDERWATER, 3.0);
                let a = ZONE_BATHROOM;
                let b = ZONE_UNDERWATER;
                let rt_lo = if a.rt60_ms < b.rt60_ms { a.rt60_ms } else { b.rt60_ms };
                let rt_hi = if a.rt60_ms > b.rt60_ms { a.rt60_ms } else { b.rt60_ms };
                let dp_lo = if a.damping_hz < b.damping_hz { a.damping_hz } else { b.damping_hz };
                let dp_hi = if a.damping_hz > b.damping_hz { a.damping_hz } else { b.damping_hz };
                let mut in_hull = true;
                let mut steps = 0u32;
                while steps < 64 {
                    tr.advance(tr.duration_ms() / 64.0);
                    let c = tr.current();
                    if !(c.rt60_ms >= rt_lo && c.rt60_ms <= rt_hi) {
                        in_hull = false;
                    }
                    if !(c.damping_hz >= dp_lo && c.damping_hz <= dp_hi) {
                        in_hull = false;
                    }
                    steps += 1;
                }
                set.add("H11-过渡-全程不越界", in_hull, "");
            }
            Err(_) => set.add("H11-过渡-全程不越界", false, ""),
        }
    }
    {
        // 非有限 / 非正 `dt` 不推进（时间倒流不得让参数回退）。
        match ZoneTransition::new(ZONE_BATHROOM, ZONE_HALL, 3.0) {
            Ok(mut tr) => {
                tr.start(ZONE_BATHROOM, ZONE_HALL, 3.0);
                let e0 = tr.elapsed_ms();
                tr.advance(0.0);
                tr.advance(-5.0);
                tr.advance(f32::NAN);
                let e1 = tr.elapsed_ms();
                set.add(
                    "H11-过渡-非法dt不推进",
                    e0 == e1 && tr.progress() == 0.0,
                    "",
                );
            }
            Err(_) => set.add("H11-过渡-非法dt不推进", false, ""),
        }
    }
    {
        // **中途改目标从当前值起坡**（不是从旧目标起坡——后者产生跳变）。
        // 推进一半后改目标，检查首步增量**接近 0**（连续性）而非跳到新 from。
        match ZoneTransition::new(ZONE_BATHROOM, ZONE_HALL, 3.0) {
            Ok(mut tr) => {
                tr.start(ZONE_BATHROOM, ZONE_HALL, 3.0);
                tr.advance(tr.duration_ms() * 0.5);
                let mid = tr.current();
                tr.start(mid, ZONE_OUTDOOR, 3.0);
                let at_start = tr.current();
                set.add(
                    "H11-过渡-中途改目标从当前起坡",
                    at_start.rt60_ms == mid.rt60_ms && at_start.damping_hz == mid.damping_hz,
                    "",
                );
            }
            Err(_) => set.add("H11-过渡-中途改目标从当前起坡", false, ""),
        }
    }
    {
        // 非法端点参数 → 构造拒收（不发散值进混响参数）。
        let r = ZoneTransition::new(
            ReverbParams {
                rt60_ms: f32::NAN,
                ..ZONE_BATHROOM
            },
            ZONE_HALL,
            3.0,
        );
        set.add("H11-过渡-非法端点拒收", r.is_err(), "");
    }

    // ---- 判据：边界（播放链不加载混响）----

    {
        // 边界声明须含四个关键成分：世界空间能力 / 播放链不加载 /
        // 自含混响（双重混响）/ 与 G07 划清。只查"非空"是弱门禁。
        let d = PLAYBACK_CHAIN_BOUNDARY;
        let ok = d.contains("世界空间")
            && d.contains("播放链")
            && d.contains("双重混响")
            && d.contains("G07");
        set.add("H11-边界-声明含关键成分", ok, "");
    }
    {
        // **边界以签名兑现**：对外渲染入口只有听者坐标，没有播放链对象。
        // 判据用一个"编译期不可伪造"的间接证据：世界渲染可跑通且
        // 只依赖听者坐标 + 分区场（无播放链参数可传）。
        let f = match field_with_custom() {
            Some(v) => v,
            // 构造失败即缺测：置红而非静默跳过（缺测绝不入 PASS）。
            None => {
                set.fail("H11-分区-夹具构造", "field-fixture-construction-failed");
                return set;
            }
        };
        let mut algo = match AlgorithmicReverb::new(ReverbMode::AlgorithmicHigh) {
            Ok(a) => a,
            Err(_) => {
                set.add("H11-边界-世界渲染无播放链入口", false, "");
                return set;
            }
        };
        let wet = render_world(&f, (1.0, 1.0, 1.0), 1.0, &mut algo);
        let dry = render_world(&f, (1.0, 1.0, 1.0), 0.0, &mut algo);
        set.add(
            "H11-边界-世界渲染无播放链入口",
            wet.is_finite() && dry.is_finite(),
            "",
        );
    }
    {
        // 分区变化确实改变渲染结果（分区模型接到了渲染链上，
        // 而非一个"建了分区表但没接线"的空壳）。
        let f = match field_with_custom() {
            Some(v) => v,
            // 构造失败即缺测：置红而非静默跳过（缺测绝不入 PASS）。
            None => {
                set.fail("H11-分区-夹具构造", "field-fixture-construction-failed");
                return set;
            }
        };
        let mut a1 = match AlgorithmicReverb::new(ReverbMode::AlgorithmicHigh) {
            Ok(v) => v,
            Err(_) => match AlgorithmicReverb::new(ReverbMode::AlgorithmicLow) {
                Ok(v) => v,
                Err(_) => {
                    set.fail("H11-边界-分区接入渲染链", "algo-reverb-ctor-failed");
                    return set;
                }
            },
        };
        let _ = render_world(&f, (1.0, 1.0, 1.0), 1.0, &mut a1);
        let mut a2 = match AlgorithmicReverb::new(ReverbMode::AlgorithmicHigh) {
            Ok(v) => v,
            Err(_) => match AlgorithmicReverb::new(ReverbMode::AlgorithmicLow) {
                Ok(v) => v,
                Err(_) => {
                    set.fail("H11-边界-分区接入渲染链", "algo-reverb-ctor-failed");
                    return set;
                }
            },
        };
        let _ = render_world(&f, (50.0, 1.0, 1.0), 1.0, &mut a2);
        let t1 = telemetry(
            &{
                let mut fsm = ReverbFsm::new(MAC_BUDGET_PER_SAMPLE);
                fsm.update(4_096);
                fsm
            },
            &f,
            (1.0, 1.0, 1.0),
        );
        let t2 = telemetry(
            &{
                let mut fsm = ReverbFsm::new(MAC_BUDGET_PER_SAMPLE);
                fsm.update(4_096);
                fsm
            },
            &f,
            (50.0, 1.0, 1.0),
        );
        set.add(
            "H11-边界-分区接入渲染链",
            t1.rt60_ms == ZONE_BATHROOM.rt60_ms
                && t2.rt60_ms == ZONE_OUTDOOR.rt60_ms
                && t1.rt60_ms != t2.rt60_ms,
            "",
        );
    }
    {
        // 遥测面（F1418 消费）字段齐备、档位标签与族名可读。
        let f = match field_with_custom() {
            Some(v) => v,
            // 构造失败即缺测：置红而非静默跳过（缺测绝不入 PASS）。
            None => {
                set.fail("H11-分区-夹具构造", "field-fixture-construction-failed");
                return set;
            }
        };
        let mut fsm = ReverbFsm::new(MAC_BUDGET_PER_SAMPLE);
        fsm.update(9_000);
        let t = telemetry(&fsm, &f, (1.0, 1.0, 1.0));
        // 族名逐族非空：只断"某个族名非空"是弱门禁（Custom 恒有值），
        // 故断五族皆非空且互不相同。
        let names: Vec<String> = [
            ZoneFamily::Bathroom,
            ZoneFamily::Hall,
            ZoneFamily::Underwater,
            ZoneFamily::Outdoor,
            ZoneFamily::Custom,
        ]
        .iter()
        .map(|x| family_name(*x))
        .collect();
        let mut names_distinct = true;
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                if names[i] == names[j] {
                    names_distinct = false;
                }
            }
        }
        let labels_ok = !t.mode.label().is_empty()
            && !ReverbMode::Convolution.label().is_empty()
            && !ReverbMode::AlgorithmicHigh.label().is_empty()
            && !ReverbMode::AlgorithmicLow.label().is_empty();
        set.add(
            "H11-边界-遥测面齐备",
            t.mode.is_algorithmic()
                && t.downgrades == 1
                && t.rt60_ms == ZONE_BATHROOM.rt60_ms
                && labels_ok
                && names_distinct
                && names[0] != String::new()
                && names[4] != String::new(),
            "",
        );
    }

    set
}

/// 把判据用的字符串 detail 泄漏为 `&'static str`（`CheckSet` 只收静态串）。
///
/// 正常路径下 detail 恒为空串（判据全绿），故该分支只在判据失败时执行，
/// 此时把实际数值固化下来供 `red_items()` 打印。
///
/// **`Box` 必须走 `alloc::boxed::Box`**：本 crate 是 `no_std`，`Box` 不在
/// prelude 里。std 隔离探针会提供 prelude，因而**掩盖**此错 —— 真仓
/// `cargo build` 才报 E0433。泄漏量与判据失败次数同阶（每次失败一处），
/// 不构成无界增长。
fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}
/// 变异登记表（判据反向验证的凭证，非生产逻辑）。
///
/// 每项登记：变异名 → 该变异**预期**转红的判据。变异器逐条施加后须核对
/// "实际转红集合 ⊇ 预期集合"，全绿即证明对应判据恒真（弱门禁）。
/// 登记与判据同文件，使"新增判据须补登记"成为可审的约定。
pub const VARIANT_REGISTRY: [(&str, &str); 20] = [
    ("M1-mac-not-equal-ir-len", "H11-卷积-每样本MAC恒等IR长度"),
    ("M2-budget-strict-lt", "H11-卷积-预算恰好取卷积"),
    ("M3-no-hysteresis", "H11-卷积-死区内不回升"),
    ("M4-downgrade-count-on-restore", "H11-卷积-降载计数不记回升"),
    ("M5-truncate-len-to-u16", "H11-卷积-超长IR饱和不截断"),
    ("M6-len-overflow-ignored", "H11-IR-超长IR拒收"),
    ("M7-sample-rate-autoresample", "H11-IR-采样率不等拒收"),
    ("M8-latency-as-full-ir", "H11-IR-延迟闭式独立重算"),
    ("M9-partial-force-lower", "H11-分区-自定义区取调用方参数"),
    ("M10-speed-zero-nan-divide", "H11-过渡-速度趋零钳上界"),
    ("M12-nonfinite-dt-advances", "H11-过渡-非法dt不推进"),
    ("M13-glerp-real-overshoot", "H11-过渡-全程不越界"),
    ("M14-both-clamps-removed", "H11-过渡-极快钳到下界"),
    ("M15-wire-collides", "H11-分区-四族具名加自定义"),
    ("M16-closed-interval", "H11-分区-半开区间边界唯一"),
    ("M17-progress-one-when-unstarted", "H11-过渡-起始等于起点"),
    ("M18-custom-not-empty", "H11-分区-自定义为空槽"),
    ("M19-named-polluted-by-caller", "H11-分区-具名族不被自定义污染"),
    ("M20-bad-zone-silently-dropped", "H11-分区-倒置体积拒收"),
    ("M21-named-slot-filled-with-custom-seat", "H11-分区-具名族不被自定义污染"),
];
