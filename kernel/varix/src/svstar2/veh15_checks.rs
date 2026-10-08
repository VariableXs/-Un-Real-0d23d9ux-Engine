//! VE-F1414 · 引擎级响度归一服务 —— 域自检判据
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1414`
//!
//! **锚点判据六条逐条映射**：批量扫描、预设表、指纹缓存、增量、报告、判据。
//!
//! ---
//!
//! ## 本文件的判据纪律
//!
//! - **判据区零 panic 面**：无 `unwrap`/`expect`/裸下标。被测实现改坏时
//!   判据必须转红并指出是哪一条，而不是自己先崩。
//! - **期望值独立写死**：常量、目标值、条数全部在判据侧另写一份，
//!   不从被测模块引用（否则「实现改了常量、判据跟着变」= 恒绿）。
//! - **每个"防退化"设计点都配反例判据**：只断"新版命中新键"时，
//!   "根本不用缓存"也能过——故必须同时断"同版命中旧键"。

use crate::checks::CheckSet;
use crate::svstar2::veh15_loudness_service::*;

// 只引入实际用到的：`String` 用于预设 id，`Vec` 用于诊断行数组。
// `format!` 与 `ToString` **不在此引入**——本层零处使用（`format!`
// 对 `ToString` 的依赖走全限定路径，显式 `use` 即死导入）。
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 判据侧独立常量（不复用被测常量）
// ---------------------------------------------------------------------------

const EXP_FP_BYTES: usize = 16;
const EXP_ALGO_V: u32 = 3;
const EXP_R128_LUFS: f32 = -23.0;
const EXP_R128_PEAK: f32 = -1.0;
const EXP_STREAM_LUFS: f32 = -14.0;
const EXP_BUILTIN_ROWS: usize = 2;
const EXP_GATE: f32 = -70.0;
const EXP_GAIN_MAX: f32 = 24.0;
const EXP_PEAK_MAX: f32 = 0.0;

/// 判据侧写的测量闭包：按路径末字符给确定值（不依赖被测逻辑）。
fn fake_measure(f: &LibraryFile) -> LoudnessReading {
    let last = f.path.as_bytes();
    if last.is_empty() {
        return LoudnessReading::new(-30.0, -6.0, 10);
    }
    match last[last.len() - 1] {
        b'0' => LoudnessReading::new(-30.0, -6.0, 10),
        b'1' => LoudnessReading::new(-18.0, -1.5, 12),
        b'2' => LoudnessReading::new(-40.0, -20.0, 4),
        b'3' => LoudnessReading::new(-60.0, -30.0, 0),
        _ => LoudnessReading::new(-25.0, -8.0, 8),
    }
}

/// 静默读数（门限下无块参与）。
fn silent_reading() -> LoudnessReading {
    LoudnessReading::new(-70.0, -60.0, 0)
}

fn fp(tag: u8) -> Fingerprint {
    let mut b = [0u8; EXP_FP_BYTES];
    let mut i = 0usize;
    while i < EXP_FP_BYTES {
        b[i] = tag;
        i += 1;
    }
    Fingerprint(b)
}

// ---------------------------------------------------------------------------
// A族：预设表与目标域
// ---------------------------------------------------------------------------

pub fn run_veh15_checks_a() -> CheckSet {
    let mut cs = CheckSet::new("VE-F1414-a");

    // A1 内置表恰两行且为 R128 与流媒体（锚点明列两项）。
    let t = TargetTable::builtin();
    let has_r128 = t.get("r128");
    let has_stream = t.get("streaming");
    cs.add(
        "内置预设表含 R128 与流媒体",
        t.len() == EXP_BUILTIN_ROWS && has_r128.is_some() && has_stream.is_some(),
        "内置 2 行：r128 与 streaming",
    );

    // A2 R128 目标值精确等于 −23/−1（判据侧写死，不引用被测常量）。
    let r128_ok = match has_r128 {
        Some(x) => {
            x.target_lufs == EXP_R128_LUFS && x.ceiling_dbtp == EXP_R128_PEAK
        }
        None => false,
    };
    cs.add(
        "R128 目标为 -23 LUFS / -1 dBTP",
        r128_ok,
        "r128 = -23/-1（判据侧写死期望）",
    );

    // A3 流媒体目标值精确等于 −14/−1。
    let stream_ok = match has_stream {
        Some(x) => x.target_lufs == EXP_STREAM_LUFS && x.ceiling_dbtp == EXP_R128_PEAK,
        None => false,
    };
    cs.add(
        "流媒体目标为 -14 LUFS / -1 dBTP",
        stream_ok,
        "streaming = -14/-1（判据侧写死期望）",
    );

    // A4 两预设的目标**不同**（相同则预设表形同虚设）。
    let distinct = match (has_r128, has_stream) {
        (Some(a), Some(b)) => a.target_lufs != b.target_lufs,
        _ => false,
    };
    cs.add(
        "两预设响度目标相异",
        distinct,
        "r128 与 streaming 目标不同（否则设了等于没设）",
    );

    // A5 预设表按 id 严格升序（确定性导出与查表依赖）。
    //
    //     **必须用「走过 upsert」的表来断**：[`TargetTable::builtin`]
    //     是直接 push、不调 sort 的，所以拿内置表断排序等于**什么都没断**
    //     （实测变异「排序方向反转」全绿）。排序逻辑住在 `upsert` 里，
    //     就必须造一个"经过 upsert 且插入序与排序序不同"的表来验它。
    let mut t_sorted = TargetTable::builtin();
    // 故意倒序插入：zzz 先、aaa 后⇒ 排序生效时最终必为 aaa,zzz。
    let _ = t_sorted.upsert(TableRow {
        id: String::from("zzz"),
        kind: PresetKind::Custom,
        target: LoudnessTarget::new(-19.0, -1.0),
    });
    let _ = t_sorted.upsert(TableRow {
        id: String::from("aaa"),
        kind: PresetKind::Custom,
        target: LoudnessTarget::new(-17.0, -1.0),
    });
    let rows5 = t_sorted.rows();
    // 通用升序判据：遍历全表断言**严格递增**。
    // 只断「内置两行的位置」抓不到排序方向被反转——两个固定行的相对次序
    // 在正序/降序下都能靠"位置恰为 0/1"蒙过去，必须与行的具体内容无关。
    let mut strictly_increasing = true;
    let mut i = 0usize;
    while i + 1 < rows5.len() {
        let prev = rows5[i].id.as_str();
        let next = rows5[i + 1].id.as_str();
        // 单边符号：正向断言 prev < next，不用 !(prev >= next)
        // （后者在有相等项时也会真，而相等正是"排序键选错"的症状）。
        if !(prev < next) {
            strictly_increasing = false;
        }
        i += 1;
    }
    let first_is_aaa = rows5.len() >= 3 && rows5[0].id.as_str() == "aaa";
    cs.add(
        "预设表按 id 严格升序",
        rows5.len() == 4 && strictly_increasing && first_is_aaa,
        "倒序插入 zzz/aaa ⇒ 最终首行为 aaa 且全表严格递增（反序即红）",
    );

    // A5b 内置表两行的位置与取值（与A5 互补：A5 管排序契约，本条管具体内容）。
    let rows = t.rows();
    cs.add(
        "内置两行位置与取值确定",
        rows.len() == EXP_BUILTIN_ROWS && rows[0].id == "r128" && rows[1].id == "streaming",
        "行 0=r128、行 1=streaming（导出口径固定）",
    );

    // A6 数据驱动：新增平台只进表即可生效（锚点「平台目标随生态更新」）。
    let mut t2 = TargetTable::builtin();
    let up = t2.upsert(TableRow {
        id: String::from("podcast"),
        kind: PresetKind::Custom,
        target: LoudnessTarget::new(-16.0, -1.0),
    });
    let now_effective = t2.get("podcast").map(|x| x.target_lufs) == Some(-16.0);
    cs.add(
        "数据驱动新增平台即刻生效",
        up.is_ok() && now_effective && t2.len() == EXP_BUILTIN_ROWS + 1,
        "upsert podcast=-16 ⇒ 查得到、表长 3（不改代码即可扩展）",
    );

    // A7 非法目标必须拒收（锚点开放表的风险面）。
    let mut t3 = TargetTable::builtin();
    let bad_nan = t3.upsert(TableRow {
        id: String::from("bad_nan"),
        kind: PresetKind::Custom,
        target: LoudnessTarget::new(f32::NAN, -1.0),
    });
    let bad_peak = t3.upsert(TableRow {
        id: String::from("bad_peak"),
        kind: PresetKind::Custom,
        // True Peak > 0 dBTP 物理不可能（削波）
        target: LoudnessTarget::new(-20.0, 3.0),
    });
    let bad_loud = t3.upsert(TableRow {
        id: String::from("bad_loud"),
        kind: PresetKind::Custom,
        // −1 LUFS 必然削波
        target: LoudnessTarget::new(-1.0, -1.0),
    });
    cs.add(
        "非法目标三项全被拒收",
        bad_nan == Err(TableError::TargetNotSane)
            && bad_peak == Err(TableError::TargetNotSane)
            && bad_loud == Err(TableError::TargetNotSane)
            && t3.len() == EXP_BUILTIN_ROWS,
        "NaN / peak>0 / 目标−1LUFS 三项均拒收且表长不变",
    );

    // A8 重复 id 拒收（同名会让生效行不可预期）。
    let mut t4 = TargetTable::builtin();
    let dup = t4.upsert(TableRow {
        id: String::from("r128"),
        kind: PresetKind::Custom,
        target: LoudnessTarget::new(-30.0, -1.0),
    });
    cs.add(
        "重复预设 id 拒收",
        dup == Err(TableError::IdDuplicate) && t4.len() == EXP_BUILTIN_ROWS,
        "同名 r128 拒收（否则改了不生效且无告警）",
    );

    // A9 空 id 拒收。
    let mut t5 = TargetTable::builtin();
    let empty = t5.upsert(TableRow {
        id: String::new(),
        kind: PresetKind::Custom,
        target: LoudnessTarget::new(-20.0, -1.0),
    });
    cs.add(
        "空预设 id 拒收",
        empty == Err(TableError::IdEmpty),
        "空 id 拒收（空键查表必然miss 且难以排查）",
    );

    // A10 三种预设种类名两两相异（回执/报告要能区分）。
    cs.add(
        "三种预设种类名相异",
        PresetKind::R128.name() != PresetKind::Streaming.name()
            && PresetKind::R128.name() != PresetKind::Custom.name()
            && PresetKind::Streaming.name() != PresetKind::Custom.name(),
        "r128/streaming/custom 三名互不相同",
    );

    // A11 目标合法性判定三向（合法 / NaN / 越界）。
    let sane_ok = LoudnessTarget::new(-23.0, -1.0).sane();
    let sane_nan = LoudnessTarget::new(f32::NAN, -1.0).sane();
    let sane_hi = LoudnessTarget::new(12.0, -1.0).sane();
    let sane_peak_nan = LoudnessTarget::new(-23.0, f32::NAN).sane();
    cs.add(
        "目标合法性四向判定",
        sane_ok && !sane_nan && !sane_hi && !sane_peak_nan,
        "合法真 / LUFS NaN 假 / LUFS 越界假 / Peak NaN 假（NaN 双向都要断）",
    );

    // A12 恰好等于区间端点判合法（等号不得判越界）。
    let at_min = LoudnessTarget::new(LOUDNESS_MIN_LUFS, -1.0).sane();
    let at_max = LoudnessTarget::new(LOUDNESS_MAX_LUFS, EXP_PEAK_MAX).sane();
    let below_min = LoudnessTarget::new(
        LOUDNESS_MIN_LUFS - 0.5,
        -1.0,
    )
    .sane();
    cs.add(
        "区间端点等号判合法",
        at_min && at_max && !below_min,
        "恰等于 -70/-5 合法、低于下界非法（等号语义）",
    );

    // A13 表导出含全部 id 与目标值（报告自解释依赖）。
    let dump = t.dump();
    cs.add(
        "预设表导出口径完整",
        dump.contains("r128") && dump.contains("streaming") && dump.contains("-23"),
        "dump 含两 id 与 -23 目标（离开模块仍可读）",
    );

    cs
}

// ---------------------------------------------------------------------------
// B族：缓存与增益
// ---------------------------------------------------------------------------

pub fn run_veh15_checks_b() -> CheckSet {
    let mut cs = CheckSet::new("VE-F1414-b");

    // B1 同版同指纹命中（缓存的基本价值）。
    let mut c = LoudnessCache::new();
    let k = CacheKey::new(fp(1), EXP_ALGO_V);
    let r = LoudnessReading::new(-22.0, -3.0, 9);
    c.insert(k, r);
    cs.add(
        "同版同指纹命中缓存",
        c.lookup(k) == Some(r),
        "写入后同键查得同值（不重复扫描的基本保证）",
    );

    // B2 **版本入键**（本单最关键的一条，设计要点一）。
    //    正向：同指纹换版本 ⇒ 必须 miss（否则返回旧口径值，
    //          即"算法升级后界面显示新版、值却是旧口径"）。
    let k_new = CacheKey::new(fp(1), EXP_ALGO_V + 1);
    let miss_new = c.lookup(k_new).is_none();
    //    反向：模拟"错误实现"——把同一指纹以**不含版本**的键
    //    （algo_version=0）写入。若版本确实参与键，两个键**必须**
    //    是不同条目（互不命中）；若版本没入键，两者会互相覆盖，
    //    后写者把先写者的值顶掉。
    //    注意不能用"仅指纹键能命中 v3 插入的条目"当反例——
    //    那是要求 version=0 等于 version=3，自相矛盾。
    let mut c2 = LoudnessCache::new();
    c2.insert(CacheKey::new(fp(1), EXP_ALGO_V), LoudnessReading::new(-30.0, -6.0, 9));
    c2.insert(
        CacheKey::fingerprint_only(fp(1)),
        LoudnessReading::new(-12.0, -2.0, 3),
    );
    let separate = c2.len() == 2
        && c2.lookup(CacheKey::new(fp(1), EXP_ALGO_V)).map(|x| x.integrated_lufs) == Some(-30.0)
        && c2.lookup(CacheKey::fingerprint_only(fp(1))).map(|x| x.integrated_lufs)
            == Some(-12.0);
    cs.add(
        "算法版本参与缓存键（升级即失效）",
        miss_new && separate,
        "v4 键未命中；v3 键与无版本键并存互不覆盖⇒版本确实入键",
    );

    // B3 指纹不同不命中（键的另一半也生效）。
    let k_diff = CacheKey::new(fp(2), EXP_ALGO_V);
    cs.add(
        "不同指纹不命中",
        c.lookup(k_diff).is_none(),
        "换指纹即 miss（否则改了内容还返回旧读数）",
    );

    // B4 同键二次插入为覆盖而非追加（纯记忆语义）。
    let mut c4 = LoudnessCache::new();
    let k4 = CacheKey::new(fp(3), EXP_ALGO_V);
    c4.insert(k4, LoudnessReading::new(-20.0, -2.0, 5));
    c4.insert(k4, LoudnessReading::new(-21.0, -2.5, 5));
    let got = c4.lookup(k4);
    cs.add(
        "同键二次插入为覆盖",
        c4.len() == 1 && got.map(|x| x.integrated_lufs) == Some(-21.0),
        "两插一条、值为后者（否则缓存会随重复扫描膨胀）",
    );

    // B5 键展示形含版本前缀（可复现，报告/日志要对得上）。
    let rendered = k.render();
    cs.add(
        "缓存键展示含版本前缀",
        rendered.starts_with("v3:") && rendered.contains("01"),
        "render = v3:0101...（版本可读，便于排查旧缓存）",
    );

    // B6 增益= 目标 − 实测（方向不得反）。
    let tg = LoudnessTarget::new(-14.0, -1.0);
    let rd = LoudnessReading::new(-20.0, -6.0, 10);
    let g = compute_gain(&rd, &tg);
    let expect_gain: f32 = -14.0 - (-20.0);
    cs.add(
        "增益方向为目标减实测",
        g.gain_db == expect_gain && !g.silent,
        "目标-14、实测-20 ⇒ 增益 +6dB（反向则越调越小声）",
    );

    // B7 超上限被钳（不许无限抬）。
    let g7 = compute_gain(
        &LoudnessReading::new(-70.0, -30.0, 5),
        &LoudnessTarget::new(-14.0, -1.0),
    );
    cs.add(
        "增益超上限被钳",
        g7.gain_db == EXP_GAIN_MAX,
        "-70 ⇒ -14 需 +56dB，钳到 +24（抬底噪无意义）",
    );

    // B8 增益下钳方向。
    let g8 = compute_gain(
        &LoudnessReading::new(-6.0, -3.0, 5),
        &LoudnessTarget::new(-70.0, -1.0),
    );
    cs.add(
        "增益低于下限时钳位",
        g8.gain_db == -EXP_GAIN_MAX,
        "-6 ⇒ -70 需 -64dB，钳到 -24",
    );

    // B9 静音不施加增益（设计要点：抬底噪）。
    let g9 = compute_gain(&silent_reading(), &tg);
    cs.add(
        "静音不加增益",
        g9.silent && g9.gain_db == 0.0,
        "gated_blocks=0 ⇒ 增益 0、标 silent（把 -70 抬到 -14 会炸底噪）",
    );

    // B10 True Peak 超顶 ⇒ 触发限幅且限幅量为负。
    //     读数 -20 LUFS / -0.2 dBTP，目标 -14 / -1 ⇒ 增益 +6 后峰值 5.8 超 -1。
    let g10 = compute_gain(
        &LoudnessReading::new(-20.0, -0.2, 10),
        &tg,
    );
    let peak_after: f32 = -0.2 + g10.gain_db;
    let expect_trim: f32 = -1.0 - peak_after;
    cs.add(
        "True Peak 超顶触发限幅",
        g10.limited && g10.ceiling_trim_db == expect_trim && expect_trim < 0.0,
        "增益后峰值 5.8 > -1 ⇒ 限幅 -6.8dB（限幅量与算法一致）",
    );

    // B11 未超顶 ⇒ 不限幅（否则「永远限幅」也能过 B10）。
    let g11 = compute_gain(&LoudnessReading::new(-20.0, -8.0, 10), &tg);
    cs.add(
        "峰值未超顶不限幅",
        !g11.limited && g11.ceiling_trim_db == 0.0,
        "增益后峰值 -2 ≤ -1 ⇒ 不限幅（与 B10 构成双向对照）",
    );

    // B12 恰好等于顶**不产生负限幅量**（等号语义）。
    //     只断 `!limited` 是**不够的**：`limited` 由 `trim_db < 0.0` 推出，
    //     而等号时 trim_db 恰为 0.0 ⇒ `0.0 < 0.0` 仍是 false，
    //     于是把判据里的 `>` 改成 `>=` 也照样全绿（实测 MISSED）。
    //     必须**直接断限幅量本身**：等号时它必须**恰为 0.0**，
    //     而"触发限幅"时才是负值——两者由数值本身区分，
    //     不依赖 `limited` 这个派生布尔。
    let g12 = compute_gain(&LoudnessReading::new(-20.0, -7.0, 10), &tg);
    // 增益后峰值 = -7 + 6 = -1.0，恰等于 ceiling -1.0
    let peak12: f32 = -7.0 + g12.gain_db;
    cs.add(
        "峰值恰等于顶时限幅量恰为零",
        !g12.limited && peak12 == -1.0 && g12.ceiling_trim_db == 0.0,
        "峰值 -1.0 = ceiling -1.0 ⇒ trim 恰 0.0 且 limited 假（等号不得判超）",
    );

    // B12b **限幅量与 limited 必须同向**（下沉到内部量的判据）。
    //     B12 只看输出层，而 `>` 与 `>=` 在等号处**输出逐位相同**
    //     （trim 都0.0、limited 都 false）⇒ 输出层原理上不可观测，
    //     属**真等价变异**，不是判据有洞（十诫15）。
    //     真正值得钉的是那条**可观测**的不变式：一旦限幅发生，
    //     限幅量必为负（把峰值压到 ceiling 之下 ⇒差值< 0），
    //     未发生则恰为 0。这条能抓住「限幅量算错方向」类真缺陷。
    let mut consistent = true;
    let mut probe_i = 0u32;
    while probe_i < 8 {
        let pk = -0.5 - (probe_i as f32) * 3.0;
        let g = compute_gain(&LoudnessReading::new(-20.0, pk, 10), &tg);
        // 不变式：limited 假 ⇒ trim 恰 0；limited 真 ⇒ trim 严格负
        if g.limited && !(g.ceiling_trim_db < 0.0) {
            consistent = false;
        }
        if !g.limited && g.ceiling_trim_db != 0.0 {
            consistent = false;
        }
        probe_i += 1;
    }
    cs.add(
        "限幅量与限幅标志同向",
        consistent,
        "未限幅⇒trim 恰 0、已限幅⇒trim 严格负（八档峰值扫过边界）",
    );

    // B13 静音读数判定（门限下无块）。
    cs.add(
        "静音判定依据门限块数",
        silent_reading().is_silent() && !LoudnessReading::new(-70.0, -60.0, 1).is_silent(),
        "gated_blocks=0 判静音、=1 不判（只看块数不看 LUFS 值）",
    );

    // B14 指纹构造与零指纹。
    let z = Fingerprint::zero();
    cs.add(
        "指纹零值可识别",
        z.is_zero() && !fp(1).is_zero() && fp(1).0.len() == EXP_FP_BYTES,
        "全零指纹可识别、正常指纹非零、长度 16",
    );

    // B15 指纹截断到定长（长输入不越界）。
    //     注意**不能**写 `fb.0[16..]` 这类越界切片：数组定长 16，
    //     该表达式在「实现正确」时反而 panic（切片越界）⇒ 门禁自己先崩，
    //     症状退化成「探针无输出」而非「红项可定位」。
    //     截断的正确判法是「长度恒为 16 且逐字节等于输入前缀」，
    //     越界与否由长度等式承担，不靠切片试探。
    let big = [
        1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
    ];
    let fb = Fingerprint::from_slice(&big);
    let mut tail_ok = true;
    let mut ti = 0usize;
    while ti < fb.0.len() {
        if fb.0[ti] != big[ti] {
            tail_ok = false;
        }
        ti += 1;
    }
    cs.add(
        "指纹从长输入截断到 16 字节",
        fb.0.len() == EXP_FP_BYTES && fb.0[15] == 16 && tail_ok,
        "20 字节输入取前 16、末位为 16（多出部分丢弃且不越界）",
    );

    // B16 缓存命中/未命中计数分列。
    //     注意顺序：**必须先有缓存条目**，否则查一个从未写入的指纹
    //     永远 miss，hits恒为 0，判据就成了恒假。
    let mut svc = LoudnessService::with_builtin_table();
    let probe_fp = fp(9);
    // ① 未命中 ⇒ misses 累加。
    svc.probe_cache(probe_fp, EXP_ALGO_V);
    svc.probe_cache(probe_fp, EXP_ALGO_V);
    let miss_ok = svc.cache.misses() == 2 && svc.cache.hits() == 0;
    // ② 写入后再查 ⇒ 命中并计入 hits。
    svc.cache.insert(
        CacheKey::new(probe_fp, EXP_ALGO_V),
        LoudnessReading::new(-20.0, -4.0, 7),
    );
    let hit_val = svc.probe_cache(probe_fp, EXP_ALGO_V);
    cs.add(
        "命中与未命中计数分列",
        miss_ok && svc.cache.hits() == 1 && hit_val.is_some(),
        "两次未命中计 misses、写入后查得计 hits（命中率报表的数据源）",
    );

    cs
}

// ---------------------------------------------------------------------------
// C族：扫描调度、增量与报告
// ---------------------------------------------------------------------------

pub fn run_veh15_checks_c() -> CheckSet {
    let mut cs = CheckSet::new("VE-F1414-c");

    // C1 首次扫描为全量且排全库（锚点「全量扫描只发生在首次」）。
    let mut s1 = LoudnessService::with_builtin_table();
    s1.apply_event(LibEvent::Added, &LibraryFile::new("a0", fp(1)), EXP_ALGO_V);
    s1.apply_event(LibEvent::Added, &LibraryFile::new("a1", fp(2)), EXP_ALGO_V);
    let (r1, q1) = s1.plan_scan(EXP_ALGO_V);
    cs.add(
        "首次扫描判全量且排全库",
        r1 == ScanReason::FullFirst && r1.is_full() && q1.len() == 2,
        "两文件库、首次 ⇒ FullFirst 且队列 2 条",
    );

    // C2 算法升级 ⇒ 全量重扫（锚点「算法升级失效重扫」）。
    let mut s2 = LoudnessService::with_builtin_table();
    s2.apply_event(LibEvent::Added, &LibraryFile::new("b0", fp(3)), EXP_ALGO_V);
    let rep2 = s2.run_scan(EXP_ALGO_V, &fake_measure);
    let (r2b, q2b) = s2.plan_scan(EXP_ALGO_V + 1);
    cs.add(
        "算法升级触发全量重扫",
        r2b == ScanReason::FullAlgoBump && r2b.is_full() && q2b.len() == 1 && rep2.analyzed == 1,
        "v3 扫过后改 v4 ⇒ FullAlgoBump 且队列含旧文件（不靠忘清缓存）",
    );

    // C3 同版再扫 ⇒ 增量且全部命中缓存（不重复扫描的兑现）。
    let mut s3 = LoudnessService::with_builtin_table();
    s3.apply_event(LibEvent::Added, &LibraryFile::new("c0", fp(4)), EXP_ALGO_V);
    s3.apply_event(LibEvent::Added, &LibraryFile::new("c1", fp(5)), EXP_ALGO_V);
    let first = s3.run_scan(EXP_ALGO_V, &fake_measure);
    let second = s3.run_scan(EXP_ALGO_V, &fake_measure);
    cs.add(
        "同版再扫走增量且零重复分析",
        first.analyzed == 2
            && second.reason == ScanReason::Incremental
            && !second.reason.is_full()
            && second.analyzed == 0
            && second.cache_hits == 2,
        "首轮实测 2、次轮增量实测 0 命中 2（重复扫描成本被消除）",
    );

    // C4 **新增文件才入增量队列**（锚点「新增/修改文件入扫描队列」）。
    let mut s4 = LoudnessService::with_builtin_table();
    s4.apply_event(LibEvent::Added, &LibraryFile::new("d0", fp(6)), EXP_ALGO_V);
    let _ = s4.run_scan(EXP_ALGO_V, &fake_measure);
    s4.apply_event(LibEvent::Added, &LibraryFile::new("d1", fp(7)), EXP_ALGO_V);
    let (r4, q4) = s4.plan_scan(EXP_ALGO_V);
    cs.add(
        "新增文件入增量队列",
        r4 == ScanReason::Incremental && q4.len() == 1,
        "加一个文件 ⇒ 增量队列恰 1 条（只扫新增，不重扫全库）",
    );

    // C5 **修改文件同样入队**（最易漏的一条：只认新增会漏掉改过的）。
    let mut s5 = LoudnessService::with_builtin_table();
    s5.apply_event(LibEvent::Added, &LibraryFile::new("e0", fp(8)), EXP_ALGO_V);
    let _ = s5.run_scan(EXP_ALGO_V, &fake_measure);
    s5.apply_event(LibEvent::Modified, &LibraryFile::new("e0", fp(9)), EXP_ALGO_V);
    let (r5, q5) = s5.plan_scan(EXP_ALGO_V);
    cs.add(
        "修改文件入增量队列",
        r5 == ScanReason::Incremental && q5.len() == 1,
        "同路径换指纹修改 ⇒ 队列 1 条（改过的文件最需要重量）",
    );

    // C5b 反向对照：修改项**分析过后**队列必须清空
//      （否则「永远全排」也能过 C5——那条只证明改动进了队列，
//        这条才证明队列不是恒等于全库）。
let rep5b = s5.run_scan(EXP_ALGO_V, &fake_measure);
    let (r5c, q5c) = s5.plan_scan(EXP_ALGO_V);
    cs.add(
        "增量队列分析后清空",
        q5c.len() == 0 && r5c == ScanReason::Incremental && rep5b.analyzed == 1,
        "改后实测 1 条⇒再次规划队列 0 条（队列不是恒等于全库）",
    );

    // C6 修改事件作废旧指纹缓存（防「改了内容哈希未变」拿到旧值）。
    let mut s6 = LoudnessService::with_builtin_table();
    let same_fp = fp(10);
    s6.apply_event(LibEvent::Added, &LibraryFile::new("f0", same_fp), EXP_ALGO_V);
    let rep6 = s6.run_scan(EXP_ALGO_V, &fake_measure);
    let cached_before = rep6.cache_hits == 0 && s6.cache.len() == 1;
    s6.apply_event(LibEvent::Modified, &LibraryFile::new("f0", same_fp), EXP_ALGO_V);
    let evicted = s6.cache.lookup(CacheKey::new(same_fp, EXP_ALGO_V)).is_none();
    cs.add(
        "修改事件作废旧指纹缓存",
        cached_before && evicted,
        "同指纹修改后旧读数被逐出（否则改内容仍返回旧 LUFS）",
    );

    // C7 删除事件出库且不入分析队列（没有内容可测）。
    let mut s7 = LoudnessService::with_builtin_table();
    s7.apply_event(LibEvent::Added, &LibraryFile::new("g0", fp(11)), EXP_ALGO_V);
    let _ = s7.run_scan(EXP_ALGO_V, &fake_measure);
    s7.apply_event(LibEvent::Removed, &LibraryFile::new("g0", fp(11)), EXP_ALGO_V);
    cs.add(
        "删除事件出库不入队",
        s7.library_len() == 0
            && !LibEvent::Removed.needs_analysis()
            && LibEvent::Added.needs_analysis()
            && LibEvent::Modified.needs_analysis(),
        "删除后库空、且删除不入分析队列（新增/修改才入）",
    );

    // C8 同路径重复添加不产生重复行（清单须唯一）。
    let mut s8 = LoudnessService::with_builtin_table();
    s8.apply_event(LibEvent::Added, &LibraryFile::new("h", fp(12)), EXP_ALGO_V);
    s8.apply_event(LibEvent::Added, &LibraryFile::new("h", fp(13)), EXP_ALGO_V);
    cs.add(
        "同路径重复添加不重复入清单",
        s8.library_len() == 1,
        "同路径两次 Added ⇒ 清单 1 行（否则报告同一文件出现两次）",
    );

    // C9 扫描原因三态可区分（首次/升级/增量）。
    cs.add(
        "扫描原因三态可区分",
        ScanReason::FullFirst != ScanReason::FullAlgoBump
            && ScanReason::FullFirst.is_full()
            && ScanReason::FullAlgoBump.is_full()
            && !ScanReason::Incremental.is_full()
            && ScanReason::FullFirst.name() != ScanReason::FullAlgoBump.name(),
        "三原因两两相异、全量恰两个（首次/升级）",
    );

    // C10 报告生成与条目数守恒。
    let mut s10 = LoudnessService::with_builtin_table();
    s10.apply_event(LibEvent::Added, &LibraryFile::new("i0", fp(14)), EXP_ALGO_V);
    s10.apply_event(LibEvent::Added, &LibraryFile::new("i1", fp(15)), EXP_ALGO_V);
    s10.apply_event(LibEvent::Added, &LibraryFile::new("i3", fp(16)), EXP_ALGO_V);
    let rep10 = s10.run_scan(EXP_ALGO_V, &fake_measure);
    let table = TargetTable::builtin();
    let built = build_report(&rep10.readings, &table, "streaming", EXP_ALGO_V);
    let ok10 = match &built {
        Some(r) => {
            r.entries.len() == rep10.readings.len()
                && r.body_lines() == 3
                && r.meta.total_files == 3
        }
        None => false,
    };
    cs.add(
        "报告条目数与读数守恒",
        ok10,
        "3 条读数 ⇒ 报告 3 条（漏行会让治理清单不可信）",
    );

    // C11 报告口径头自带预设/版本/门限（自解释，设计要点五）。
    let text = match &built {
        Some(r) => r.export(),
        None => String::new(),
    };
    cs.add(
        "报告导出含口径头",
        text.contains("preset=streaming")
            && text.contains("algo=v3")
            && text.contains("gate=-70")
            && text.contains("files=3"),
        "首行含 preset/algo/gate/files（离开模块仍自解释）",
    );

    // C12 未知预设不产报告（不悄悄用默认值）。
    let missing = build_report(&rep10.readings, &table, "no_such_preset", EXP_ALGO_V);
    cs.add(
        "未知预设不产报告",
        missing.is_none(),
        "查不到预设 ⇒ None（整张报告的基准错了，不该降级为默认）",
    );

    // C13 静音文件在报告中被标 SILENT 且不计增益。
    //     路径末字符须为 '3'——`fake_measure` 据此返回 gated_blocks=0
    //     （用 '0' 会拿到 blocks=10，判据就成了"期望静音但语料不静音"
    //     的自相矛盾，症状是恒红而非报出真缺陷）。
    let mut s13 = LoudnessService::with_builtin_table();
    s13.apply_event(LibEvent::Added, &LibraryFile::new("j3", fp(17)), EXP_ALGO_V);
    let rep13 = s13.run_scan(EXP_ALGO_V, &fake_measure);
    let r13 = build_report(&rep13.readings, &table, "r128", EXP_ALGO_V);
    let silent_ok = match &r13 {
        Some(r) => {
            r.meta.silent_files == 1
                && r.entries.len() == 1
                && r.entries.first().map(|e| e.silent) == Some(true)
                && r.entries.first().map(|e| e.gain_db) == Some(0.0)
                && r.export().contains("SILENT")
        }
        None => false,
    };
    cs.add(
        "静音文件标记且零增益",
        silent_ok,
        "gated_blocks=0 ⇒ silent_files=1、增益 0、导出含 SILENT",
    );

    // C14 限幅计数与条目自洽（负向与正向都要）。
    let mut s14 = LoudnessService::with_builtin_table();
    s14.apply_event(LibEvent::Added, &LibraryFile::new("k1", fp(18)), EXP_ALGO_V);
    s14.apply_event(LibEvent::Added, &LibraryFile::new("k2", fp(19)), EXP_ALGO_V);
    let rep14 = s14.run_scan(EXP_ALGO_V, &fake_measure);
    let r14 = build_report(&rep14.readings, &table, "streaming", EXP_ALGO_V);
    let lim = match &r14 {
        Some(r) => {
            let n = r.limited_count();
            let mut manual = 0u32;
            let mut i = 0usize;
            while i < r.entries.len() {
                if r.entries[i].limited {
                    manual = manual.saturating_add(1);
                }
                i += 1;
            }
            n == manual
        }
        None => false,
    };
    cs.add(
        "限幅计数与条目自洽",
        lim,
        "limited_count 等于逐条统计（口径错则治理清单错）",
    );

    // C15 诊断聚合含中英双语且行数固定。
    let empty_rep = ScanReport {
        reason: ScanReason::Incremental,
        analyzed: 0,
        cache_hits: 0,
        queued: 0,
        readings: Vec::new(),
    };
    let lines = diagnostic_lines(&s10, &empty_rep);
    let bi = lines.iter().any(|l| l.contains("库文件") && l.contains("library"));
    cs.add(
        "诊断聚合三行且中英双语",
        lines.len() == 3 && bi,
        "3 行聚合，首行同时含「库文件/library」",
    );

    // C16 **静音不施增益**（接手补录：实现有 `compute_gain` 的静音短路，
    // 判据层原本零覆盖 ⇒ 一个「删掉短路、把 −70 抬到 −14」的回归
    // 会全绿放过，而那会把底噪放大 56 dB）。
    //
    // **双向**：静音 ⇒ 增益恰 0 且 `silent` 标记为真；非静音 ⇒
    // `silent` 为假。只断前者的话「永远返回 gain=0」也过；
    // 只断后者的话「永远标记 silent」也过。
    let silent_reading =
        LoudnessReading::new(EXP_GATE, EXP_PEAK_MAX, 0);
    let loud_reading = LoudnessReading::new(-30.0, -1.0, 8);
    let t_r128 = LoudnessTarget::new(EXP_STREAM_LUFS, -1.0);
    let g_sil = compute_gain(&silent_reading, &t_r128);
    let g_loud = compute_gain(&loud_reading, &t_r128);
    cs.add(
        "静音片不施增益",
        silent_reading.is_silent()
            && g_sil.silent
            && g_sil.gain_db == 0.0
            && g_sil.ceiling_trim_db == 0.0
            && !g_sil.limited,
        "整片静音（0 块入积分）⇒ gain 恰 0、trim 恰 0、不限幅、silent \
         为真——把 −70 抬到 −14 会把底噪放大 56 dB",
    );
    cs.add(
        "非静音片正常施增益",
        !loud_reading.is_silent() && !g_loud.silent && g_loud.gain_db != 0.0,
        "−30 LUFS 配 −14 目标 ⇒ silent 为假且增益非 0（**反向**：只断 \
         「静音不增益」的话「一律返回 0」的实现也过）",
    );

    // C17 **静音门限常量契约**（判据侧独立写死 −70，不引用被测常量）。
    // 门限若被改成 −30，低于 −30 的素材会被当静音而不参与归一，
    // 报告上却看不出任何异常。
    cs.add(
        "静音门限为 -70 LUFS",
        SILENCE_GATE_LUFS == EXP_GATE,
        "SILENCE_GATE_LUFS 恰为 −70（判据侧独立写死，不复用被测常量——\
         复用即「实现改常量、判据跟着变」= 恒绿）",
    );

    cs
}

// ---------------------------------------------------------------------------
// 独立入口（三族分立，规避 MAX_CHECKS 截断）
// ---------------------------------------------------------------------------

pub fn run_veh15_checks_a_standalone() -> CheckSet {
    run_veh15_checks_a()
}

pub fn run_veh15_checks_b_standalone() -> CheckSet {
    run_veh15_checks_b()
}

pub fn run_veh15_checks_c_standalone() -> CheckSet {
    run_veh15_checks_c()
}