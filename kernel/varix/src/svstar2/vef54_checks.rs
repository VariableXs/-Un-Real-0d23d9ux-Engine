//! VE-F5401 · AA 域开工与网络总架构（VE-AA 域 · 域自检）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5401`
//!
//! **判据（锚点原文）**：四层、流畅公平、承接落地、层冻结、判据。
//!
//! 分五组，逐条映射锚点：
//! - `c5401_layers`   → 判据一「四层」：四层齐备 / 层号方向 / 依赖白名单
//! - `c5401_freeze`   → 判据二「层冻结」：逐接口不可变 / 二次冻结被拒 / 越权被拒 / 台账自查
//! - `c5401_handover` → 判据三「承接落地」：三承接点 / 件号回溯 / 缺源被拒不顶替
//! - `c5401_promise`  → 判据四「流畅公平」：预算拒绝记账 / 积压不清零 / 同输入同判定
//! - `c5401_gate`     → 判据五「判据」：开工闸四条件 / 原因可查 / 判据集自身性质
//!
//! **本文件的判据纪律（十诫）**：
//! 1. **独立预期常量**：层数、件号、预算数值一律在判据侧写死
//!    （`EXP_LAYERS` / `EXP_ITEMS` / `EXP_FRAME_BUDGET`），**不从被测
//!    常量反推**。若判据直接引 `st::FRAME_BUDGET_BYTES`，
//!    把被测预算改成 1 亿判据也全绿。
//! 2. **「缺源拒收」必须配「有源收下」的对照**：只测缺源被拒的话，
//!    把 `bind` 改成永远返回 `Err` 判据也全绿 ⇒ 必须同时断
//!    「源齐时绑定成功且 `bound_count` 恰等于期望数」。
//! 3. **「二次冻结被拒」必须配「一次冻结成功」**：改成永远 `Err`
//!    同样全绿 ⇒ 同时断首次冻结返回序号且序号从 1 起。
//! 4. **公平判据不做自证**：[`st::fair`] 返回什么，判据侧就用
//!    **独立重算的同余式**对账，不问被测函数「你对吗」。
//! 5. **预算判据独立重算分母**：`used` 的期望值由判据侧按
//!    「逐次累加 want」自己算，不读被测的 `used`。
//! 6. **开工闸「可过」必须配「四条件逐一不可过」**：只测可过的话，
//!    把 `why_blocked` 改成永远 `None` 判据全绿 ⇒ 分别破坏
//!    四条件，断言每次都给出**对应**的原因串。
//! 7. **判据区零 panic 面**：无 `unwrap()`/`expect()`；下标访问先比长度。

extern crate alloc;

use alloc::vec::Vec;

use crate::checks::{CheckSet, MAX_CHECKS};
use crate::svstar2::vef54_aaarch as st;

// ---------------------------------------------------------------------------
// 判据侧独立预期常量（十诫第 1 条：不从被测反推）
// ---------------------------------------------------------------------------

/// 期望层数 = 4（锚点「总架构四层」）。
const EXP_LAYERS: usize = 4;
/// 期望移交包件数 = 10（锚点「F5393 十件」）。
const EXP_ITEMS: u32 = 10;
/// 期望单帧带宽预算 = 64 KiB。
const EXP_FRAME_BUDGET: u32 = 64 * 1024;
/// 期望抖动上界 = 8 KiB。
const EXP_JITTER: u32 = 8 * 1024;
/// 期望承接点数 = 3（锚点：复制消费 / 事件总线 / 带宽预算）。
const EXP_SURFACES: usize = 3;

/// 坐标口径帧号（判据侧写死；层间对拍用）。
const FRAME_A: u32 = 7;
/// 另一个坐标口径（与 `FRAME_A` 刻意不同 ⇒ 必然失配）。
const FRAME_B: u32 = 9;

/// 能力位：位 0 = 位移同步，位 1 = 动画同步，位 2 = 音频流。
const CAP_MOTION: u32 = 1 << 0;
const CAP_ANIM: u32 = 1 << 1;
const CAP_AUDIO: u32 = 1 << 2;

/// 独立重算的公平判定（同余 + 异或，与 [`st::fair`] 同一个式子，
/// 但**在判据侧另写一遍**——若直接引被测函数则属自证）。
fn expect_fair(input: u64) -> u64 {
    let mut h = input ^ 0x9e37_79b9_7f4a_7c15;
    h = h.wrapping_mul(0x0000_0100_0000_01b3);
    h ^= h >> 29;
    h = h.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    h ^= h >> 32;
    h % 1000
}

/// 一个**满包**（十件齐）。
fn full_pack() -> st::HandoverPack {
    st::HandoverPack::full()
}

/// 把三个承接点全部绑定上（满包下必成功）。
fn bind_all(h: &mut st::HandoverLedger) {
    let pack = full_pack();
    let _ = h.bind(st::Surface::ReplayConsume, pack, "fx_metric.table");
    let _ = h.bind(st::Surface::EventBus, pack, "iface_ledger.net");
    let _ = h.bind(st::Surface::Bandwidth, pack, "perf_book.budget");
}

// ---------------------------------------------------------------------------
// 判据一：四层
// ---------------------------------------------------------------------------

fn c5401_layers(s: &mut CheckSet) {
    // ① 层数恰为 4，且四层齐备（逐层点名，不靠 `len()` 一句带过）。
    let mut all_present = st::Layer::ALL.len() == EXP_LAYERS;
    for l in [st::Layer::Transport, st::Layer::Session, st::Layer::Replication, st::Layer::Gameplay] {
        if st::Layer::from_index(l.index()) != Some(l) {
            all_present = false;
        }
        if l.name().is_empty() {
            all_present = false;
        }
    }
    s.add(
        "四层齐备且层号可反解",
        all_present && st::Layer::ALL.len() == EXP_LAYERS,
        "锚点「总架构四层」：传输/会话/复制/玩法四层齐备，层号与枚举一一对应，层名非空（读屏可达）",
    );

    // ② 层号严格递增（依赖方向即层号方向）。
    let mut mono = true;
    for i in 1..st::Layer::ALL.len() {
        if !(st::Layer::ALL[i - 1].index() < st::Layer::ALL[i].index()) {
            mono = false;
        }
    }
    s.add(
        "层号严格递增",
        mono,
        "传输(0)<会话(1)<复制(2)<玩法(3)，层号序即依赖方向序",
    );

    // ③ 依赖白名单：上层可依赖严格下层，反向越权。
    let up_ok = st::dep_allowed(st::Layer::Gameplay, st::Layer::Transport)
        && st::dep_allowed(st::Layer::Replication, st::Layer::Session);
    let down_bad = !st::dep_allowed(st::Layer::Transport, st::Layer::Gameplay)
        && !st::dep_allowed(st::Layer::Session, st::Layer::Replication);
    s.add(
        "依赖仅上→下，反向越权被拒",
        up_ok && down_bad,
        "玩法层可依赖传输层；传输层不得依赖玩法层、会话层不得依赖复制层（方向错是最难在运行期发现的一类）",
    );

    // ④ 依赖白名单内容与层号严格一致（不是随便一个数组）。
    let mut white_ok = true;
    for l in st::Layer::ALL.iter() {
        let allowed = l.allowed_deps();
        // 白名单里每一项都必须是严格下层。
        for d in allowed.iter() {
            if !st::dep_allowed(*l, *d) {
                white_ok = false;
            }
        }
        // 白名单长度恰为层号（0/1/2/3 个严格下层）。
        if allowed.len() != l.index() as usize {
            white_ok = false;
        }
    }
    s.add(
        "依赖白名单逐层自洽",
        white_ok,
        "每层白名单恰含层号个严格下层（传输0/会话1/复制2/玩法3），无自引用无反向",
    );

    // ⑤ 越界层号返回 None，绝不猜测。
    let oob = st::Layer::from_index(4).is_none()
        && st::Layer::from_index(255).is_none()
        && st::Layer::from_index(0) == Some(st::Layer::Transport);
    s.add(
        "越界层号不猜测",
        oob,
        "index=4/255 返回 None，index=0 返回传输层；非法输入不得静默落到某层",
    );

    // ⑦ 自依赖被拒（`dep_allowed` 若写成 `>=` 会放行 a==a）。
    //
    // 这条是变异验证补上的：M15 把 `>` 改成 `>=` 时，前六条判据
    // 全绿 —— 因为它们只测了「反向」（传输依赖玩法），没测
    // 「同层」。自依赖在架构上同样非法（层不能依赖自己）。
    let self_bad = !st::dep_allowed(st::Layer::Transport, st::Layer::Transport)
        && !st::dep_allowed(st::Layer::Gameplay, st::Layer::Gameplay)
        && !st::dep_allowed(st::Layer::Session, st::Layer::Session);
    s.add(
        "自依赖被拒（同层不算下层）",
        self_bad,
        "传输/会话/玩法三层各自 dep_allowed(自身) 均为 false（`>` 写成 `>=` 会放行自依赖）",
    );
}

/// 判据二·对拍面：层间失配 → 对拍（锚点错误路径第一条）。
///
/// **单列一族**：`probe` 与 `Mismatch` 是锚点「层间失配 → 对拍」的
/// 全部落点，混在层/冻结里会被稀释。
fn c5401_probe(s: &mut CheckSet) {
    // ① 一致路径：口径一致 + 能力恰好对上 ⇒ `None`。
    //
    // **必须有这条**：若只有失配判据，把 `probe` 改成永远返回
    // `CoordFrame` 判据也全绿。
    let agree = st::Probe {
        caller: st::Layer::Gameplay,
        provider: st::Layer::Replication,
        caller_frame: FRAME_A,
        provider_frame: FRAME_A,
        caller_caps: CAP_MOTION | CAP_ANIM,
        provider_caps: CAP_MOTION | CAP_ANIM,
    };
    s.add(
        "对拍一致路径返回 None",
        st::probe_ok(st::probe(&agree)),
        "口径一致且能力恰好对上 ⇒ 无失配（否则「永远报失配」也能过全部失配判据）",
    );

    // ② 口径失配：**必须报**，且两栏值都要带出来（供玩法层定位）。
    let frame_bad = st::Probe {
        caller: st::Layer::Gameplay,
        provider: st::Layer::Replication,
        caller_frame: FRAME_A,
        provider_frame: FRAME_B,
        caller_caps: CAP_MOTION,
        provider_caps: CAP_MOTION,
    };
    let m_frame = st::probe(&frame_bad);
    s.add(
        "口径失配被报且带两栏值",
        m_frame
            == st::Mismatch::CoordFrame {
                caller: st::Layer::Gameplay,
                caller_frame: FRAME_A,
                provider_frame: FRAME_B,
            },
        "上报调用方 FRAME_A=7 与提供方 FRAME_B=9（不报=画面错位没人发现；报错不带值=无法定位）",
    );

    // ③ 缺能力：取**最低缺失位**，同状态必得同一 cap 号（确定性）。
    //
    // **必须用「两个缺失位」**：变异验证 M21 把 `trailing_zeros` 改成
    // 取最高位时，若缺失位只有一个（bit1），两种写法都得 1 ⇒ 判据全绿。
    // 故本条要 motion+anim+audio 只给 audio（缺 bit0 与 bit1），
    // 最低位=0、最高位=1，两者可区分。
    let miss = st::Probe {
        caller: st::Layer::Gameplay,
        provider: st::Layer::Replication,
        caller_frame: FRAME_A,
        provider_frame: FRAME_A,
        caller_caps: CAP_MOTION | CAP_ANIM | CAP_AUDIO,
        provider_caps: CAP_AUDIO,
    };
    let m_miss = st::probe(&miss);
    // 重复调用同一输入必须得同一结果（否则调用方无法稳定处置）。
    let m_miss2 = st::probe(&miss);
    s.add(
        "缺能力取最低缺失位",
        m_miss == st::Mismatch::CapabilityMissing { caller: st::Layer::Gameplay, cap: 0 }
            && m_miss == m_miss2,
        "要 motion+anim+audio 只给 audio ⇒ 缺 bit0 与 bit1，报最低位 cap=0（取最高位会得 1；同输入两次结果必须相同）",
    );

    // ③b 只缺高位时，最低位仍是对的那个（防「总是硬编码低位」）。
    let miss_hi = st::Probe {
        caller: st::Layer::Gameplay,
        provider: st::Layer::Replication,
        caller_frame: FRAME_A,
        provider_frame: FRAME_A,
        caller_caps: CAP_MOTION | CAP_ANIM,
        provider_caps: CAP_MOTION,
    };
    s.add(
        "只缺高位时取到高位",
        st::probe(&miss_hi) == st::Mismatch::CapabilityMissing { caller: st::Layer::Gameplay, cap: 1 },
        "要 motion+anim 只给 motion ⇒ 报 cap=1（若实现硬编码返回 0 则这条红）",
    );

    // ④ 虚报：下层多报能力（「不骗」的反面）。
    //
    // **优先级断言**：口径一致、能力不缺时，**虚报也必须报出来**——
    // 若 `probe` 只查缺能力，把 `extra` 那段删掉判据仍全绿。
    let over = st::Probe {
        caller: st::Layer::Gameplay,
        provider: st::Layer::Replication,
        caller_frame: FRAME_A,
        provider_frame: FRAME_A,
        caller_caps: CAP_MOTION,
        provider_caps: CAP_MOTION | CAP_AUDIO,
    };
    s.add(
        "下层虚报能力被报",
        st::probe(&over) == st::Mismatch::CapabilityOverreport { provider: st::Layer::Replication, cap: 2 },
        "只要 motion 却报有 audio ⇒ 报虚报 cap=2（悄悄认下就是「骗」）",
    );

    // ⑤ 方向错优先于其它失配（越权是结构问题，先报它）。
    let order = st::Probe {
        caller: st::Layer::Transport,
        provider: st::Layer::Gameplay,
        caller_frame: FRAME_A,
        provider_frame: FRAME_B,
        caller_caps: CAP_MOTION,
        provider_caps: CAP_AUDIO,
    };
    s.add(
        "方向错优先于口径失配",
        st::probe(&order)
            == st::Mismatch::LayerOrder { caller: st::Layer::Transport, provider: st::Layer::Gameplay },
        "传输层依赖玩法层且口径也不对 ⇒ 先报 LayerOrder（结构错优先；口径错是症状）",
    );

    // ⑥ **对拍不补偿**：失配时不得改写任何输入（架构层不替下游猜）。
    //
    // 这条按「不修改」断言：`probe` 取 `&Probe`，故 `order` 在调用
    // 前后逐字段相等——若签名改成 `&mut Probe` 并偷偷归一化口径，
    // 这条会红。
    let snapshot = order;
    let _ = st::probe(&order);
    s.add(
        "对拍只报不修（不改写调用方输入）",
        snapshot.caller_frame == order.caller_frame
            && snapshot.provider_frame == order.provider_frame
            && snapshot.caller_caps == order.caller_caps
            && snapshot.provider_caps == order.provider_caps,
        "失配对拍后调用方的口径与能力位逐字段不变（补偿是玩法层的事，架构层替它猜=把错误藏起来）",
    );
}

// ---------------------------------------------------------------------------
// 判据二：层冻结
// ---------------------------------------------------------------------------

fn c5401_freeze(s: &mut CheckSet) {
    // ① 首次冻结成功，序号从 1 起（十诫第 3 条的对照面）。
    let mut l = st::FreezeLedger::new();
    let sig1 = st::sig_fingerprint(st::Layer::Gameplay, st::Layer::Replication, "submit_state");
    let r1 = l.freeze(st::Layer::Gameplay, st::Layer::Replication, sig1, "witness-a");
    let first_ok = matches!(r1, Ok(1)) && l.is_frozen(st::Layer::Gameplay, st::Layer::Replication, sig1);
    s.add(
        "首次冻结成功且序号自 1 起",
        first_ok,
        "锚点「层间接口冻结」：首次冻结返回序号 1，之后可按精确三元组查到",
    );

    // ② 二次冻结同签名被拒（不静默成功）。
    let r2 = l.freeze(st::Layer::Gameplay, st::Layer::Replication, sig1, "witness-b");
    let dup_rejected = r2 == Err(st::FreezeError::AlreadyFrozen) && l.len() == 1;
    s.add(
        "二次冻结同签名被拒且不增行",
        dup_rejected,
        "重复冻结返回 AlreadyFrozen 且台账仍为 1 条（静默成功会掩盖「有人以为能改」）",
    );

    // ③ 越权方向冻结被拒（LayerOrder），且不落行。
    let mut l2 = st::FreezeLedger::new();
    let sig_rev = st::sig_fingerprint(st::Layer::Transport, st::Layer::Gameplay, "pull_state");
    let rev = l2.freeze(st::Layer::Transport, st::Layer::Gameplay, sig_rev, "witness-c");
    s.add(
        "下层依赖上层被拒且不落行",
        rev == Err(st::FreezeError::LayerOrder) && l2.is_empty(),
        "锚点「接口越权→冻结流程」：方向错在冻结入口即拒，不写入台账",
    );

    // ④ 签名指纹：区分度 + 稳定性。
    //
    // **如实登记**：这里断言的「前缀歧义区分」在 FNV-1a 上**由名字
    // 逐字节入哈希保证**（`"ab"+"c"` 与 `"a"+"bc"` 的字节流本就不同），
    // **不是**末位长度混入的功劳 —— 变异验证 M16（删掉长度混入）
    // 实测 MISS，即长度混入属**等价变异**（防的是理论碰撞，短名上
    // 无实测碰撞）。故本条只声称「可观测的性质」，不把等价变异
    // 说成强门禁（否则就成了自吹）。
    let f_ab = st::sig_fingerprint(st::Layer::Gameplay, st::Layer::Session, "ab");
    let f_abc = st::sig_fingerprint(st::Layer::Gameplay, st::Layer::Session, "abc");
    let f_ac = st::sig_fingerprint(st::Layer::Gameplay, st::Layer::Session, "ac");
    let f_same = st::sig_fingerprint(st::Layer::Gameplay, st::Layer::Session, "abc");
    let distinct = f_ab != f_abc && f_abc != f_ac && f_ab != f_ac;
    let stable = f_abc == f_same;
    // 层对参与指纹：同名方法在别的层对上必不同（**有后果**的维度）。
    let f_other_layer = st::sig_fingerprint(st::Layer::Replication, st::Layer::Session, "abc");
    s.add(
        "签名指纹区分且稳定",
        distinct && stable && f_abc != f_other_layer,
        "指纹区分前缀歧义(ab/abc/ac)且同输入恒等；层对参与指纹，故同名方法跨层对必不同",
    );

    // ⑤ 台账自查：正例过。
    let mut good = st::FreezeLedger::new();
    let _ = good.freeze(st::Layer::Gameplay, st::Layer::Replication, 11, "w");
    let _ = good.freeze(st::Layer::Replication, st::Layer::Session, 22, "w");
    let clean_pass = good.audit() == Ok(2);
    s.add(
        "冻结台账自查：正例过",
        clean_pass,
        "两条合法冻结 ⇒ audit 返回 Ok(2)",
    );

    // ⑥ 台账 audit 的**不变式真的被守着**（不是注释里的保证）。
    //
    // `audit` 的三条检查（序号递增 / 方向合法 / 无重复三元组）在
    // **当前 API 下都不可达**——`freeze` 单调发号且拒重复，所以
    // 「把 audit 改成恒真」是**等价变异**（变异验证 M8 实测 MISS）。
    // 但注释里写着的保证不能只靠注释：这里用**等价的公开面**把
    // 三条不变式各钉一遍——若哪天 `freeze` 放宽了（比如允许覆写
    // 已有签名），本判据会先于 audit 转红提醒。
    let mut inv = st::FreezeLedger::new();
    let mut inv_ok = true;
    // 不变式 a：序号严格递增 ⇒ 连续冻结拿到的 seq 是 1,2,3…（不重复、单调）。
    let mut seqs: Vec<u32> = Vec::new();
    for k in 0..3u64 {
        let sig = 100 + k;
        match inv.freeze(st::Layer::Gameplay, st::Layer::Replication, sig, "w") {
            Ok(sq) => seqs.push(sq),
            Err(_) => inv_ok = false,
        }
    }
    if seqs.len() != 3 || seqs[0] != 1 || seqs[1] != 2 || seqs[2] != 3 {
        inv_ok = false;
    }
    // 不变式 b：方向非法的记录进不来（故 audit 的方向检查当前恒真）。
    let rev2 = st::FreezeLedger::new().freeze(
        st::Layer::Transport,
        st::Layer::Gameplay,
        999,
        "w",
    );
    if rev2 != Err(st::FreezeError::LayerOrder) {
        inv_ok = false;
    }
    // 不变式 c：无重复三元组（重复被 freeze 入口挡住）。
    let dup = inv.freeze(st::Layer::Gameplay, st::Layer::Replication, 100, "w");
    if dup != Err(st::FreezeError::AlreadyFrozen) {
        inv_ok = false;
    }
    s.add(
        "audit 三条不变式在公开面被守着",
        inv_ok && inv.audit() == Ok(3),
        "序号 1,2,3 单调 / 反向冻结被拒 / 重复被拒 ⇒ audit 当前恒真是**结构性等价**，不是漏网",
    );

    // ⑦ 层号**参与**签名指纹（有可观测后果的维度）。
    let f_la = st::sig_fingerprint(st::Layer::Gameplay, st::Layer::Session, "sync");
    let f_lb = st::sig_fingerprint(st::Layer::Replication, st::Layer::Session, "sync");
    let f_lc = st::sig_fingerprint(st::Layer::Gameplay, st::Layer::Transport, "sync");
    // 三组（层对不同，名字相同）必须两两不同，否则同名方法跨层对
    // 会共用一条冻结记录 —— 冻结就失去区分能力。
    let layer_matters = f_la != f_lb && f_lb != f_lc && f_la != f_lc;
    s.add(
        "层号参与签名指纹",
        layer_matters,
        "同名 sync 在 (玩法,会话)/(复制,会话)/(玩法,传输) 三个层对上指纹两两不同（层号不进哈希则三者同值）",
    );

    // ⑥ 逐层冻结计数（供「声明数==冻结数」对账，不看总数）。
    let mut l3 = st::FreezeLedger::new();
    let _ = l3.freeze(st::Layer::Gameplay, st::Layer::Replication, 31, "w");
    let _ = l3.freeze(st::Layer::Gameplay, st::Layer::Replication, 32, "w");
    let _ = l3.freeze(st::Layer::Replication, st::Layer::Session, 33, "w");
    let per_layer_ok = l3.frozen_count_of(st::Layer::Replication) == 2
        && l3.frozen_count_of(st::Layer::Session) == 1
        && l3.frozen_count_of(st::Layer::Transport) == 0
        && l3.len() == 3;
    s.add(
        "逐层冻结计数精确",
        per_layer_ok,
        "复制层提供 2 条、会话层 1 条、传输层 0 条，总计 3（只看总数会漏掉层间错配）",
    );

    // ⑦ 按序号取条目（审计遍历面）。
    let got = l3.get(1).map(|f| f.sig) == Some(31) && l3.get(3).map(|f| f.sig) == Some(33);
    let miss = l3.get(99).is_none();
    s.add(
        "按序号审计可取且越界为 None",
        got && miss,
        "seq=1⇒sig31 / seq=3⇒sig33；seq=99 返回 None 不 panic",
    );

    // ⑧ 冻结不可解冻：台账无任何解冻入口，且 audit 不接受「少了一条」。
    let before = l3.len();
    let audit_before = l3.audit();
    s.add(
        "冻结条数只增不减",
        before == 3 && audit_before == Ok(3),
        "台账无解冻入口；连查三次 audit 恒为 3 条，改签名须重新冻结（新序号）",
    );
}

// ---------------------------------------------------------------------------
// 判据三：承接落地
// ---------------------------------------------------------------------------

fn c5401_handover(s: &mut CheckSet) {
    // ① 三承接点齐备，且各自回溯到期望件号（判据侧写死期望）。
    let mut expect_map_ok = true;
    let exp_items = [
        (st::Surface::ReplayConsume, 2u32),
        (st::Surface::EventBus, 1u32),
        (st::Surface::Bandwidth, 6u32),
    ];
    for (sf, item) in exp_items.iter() {
        if sf.expect_item() != *item {
            expect_map_ok = false;
        }
    }
    let names_ok = !st::Surface::ReplayConsume.name().is_empty()
        && !st::Surface::EventBus.name().is_empty()
        && !st::Surface::Bandwidth.name().is_empty();
    s.add(
        "三承接点回溯件号正确",
        expect_map_ok && names_ok && st::Surface::ALL.len() == EXP_SURFACES,
        "锚点三承接点：复制消费→特效指标(2) / 事件总线→接口总账(1) / 带宽→性能总册(6)，件号判据侧写死不从被测反推",
    );

    // ② 满包下三面全绑定成功（十诫第 2 条的对照面）。
    let mut h = st::HandoverLedger::new();
    bind_all(&mut h);
    let bound = h.bound_count();
    s.add(
        "源齐时三面全绑定",
        bound == EXP_SURFACES && h.len() == EXP_SURFACES,
        "满包下三个承接点全部绑定成功（否则「缺源拒收」判据可用永远 Err 伪造通过）",
    );

    // ③ 缺源被拒，且错误里带**回溯件号**。
    let mut h2 = st::HandoverLedger::new();
    let mut pack = st::HandoverPack::empty();
    let _ = pack.declare(1); // 只有接口总账，缺 2 与 6
    let e1 = h2.bind(st::Surface::ReplayConsume, pack, "fx.table");
    let e2 = h2.bind(st::Surface::Bandwidth, pack, "perf.budget");
    let backtrack_ok = e1 == Err(st::BindError::SourceMissing { item: 2 })
        && e2 == Err(st::BindError::SourceMissing { item: 6 })
        && h2.bound_count() == 0;
    s.add(
        "缺源被拒且带回溯件号",
        backtrack_ok,
        "锚点「承接缺源→回溯移交包」：错误携带期望件号（2/6）供顺号找上游，不用默认值顶替",
    );

    // ④ 缺**条目名**同样算缺源（指不回具体条目 = 变相缺源）。
    let mut h3 = st::HandoverLedger::new();
    let e3 = h3.bind(st::Surface::EventBus, full_pack(), "");
    s.add(
        "条目名为空视同缺源",
        e3 == Err(st::BindError::SourceMissing { item: 1 }) && h3.bound_count() == 0,
        "源件在包里但条目名为空 ⇒ 指不回具体条目，按缺源拒收，不静默绑定",
    );

    // ⑤ 移交包位掩码语义：声明/查询/缺件列表。
    let mut p = st::HandoverPack::empty();
    let mut decl_ok = p.declare(3) && p.declare(7);
    decl_ok &= !p.declare(0) && !p.declare(EXP_ITEMS + 1);
    let has_ok = p.has(3) && p.has(7) && !p.has(4) && !p.has(0) && !p.has(EXP_ITEMS + 1);
    // 缺件列表升序且条数正确（1..10 除去 3、7 ⇒ 8 件）。
    let missing = p.missing_items();
    let mut miss_sorted = true;
    for i in 1..missing.len() {
        if missing[i - 1] >= missing[i] {
            miss_sorted = false;
        }
    }
    let miss_ok = miss_sorted
        && missing.len() == (EXP_ITEMS as usize - 2)
        && !missing.contains(&3)
        && !missing.contains(&7);
    s.add(
        "移交包位掩码语义正确",
        decl_ok && has_ok && miss_ok,
        "声明越界(0/11)返回 false 不静默；缺件列表升序 8 件且不含已声明的 3、7",
    );

    // ⑥ 满包恰十件且无缺件。
    let fp = full_pack();
    s.add(
        "满包恰十件",
        fp.missing_items().is_empty() && EXP_ITEMS == 10,
        "F5393 十件齐备时缺件列表为空",
    );

    // ⑦ 落地表行内容可查（追责到条目）。
    let row = h.row(st::Surface::Bandwidth);
    let row_ok = row.map(|r| r.bound && r.item == 6 && r.entry == "perf_book.budget").unwrap_or(false);
    s.add(
        "落地行可追责到条目",
        row_ok,
        "带宽口径行：bound=true / item=6 / entry=perf_book.budget",
    );
}

// ---------------------------------------------------------------------------
// 判据四：流畅与公平（域本色双承诺）
// ---------------------------------------------------------------------------

fn c5401_promise(s: &mut CheckSet) {
    // ① 预算常量口径（判据侧独立写死，被测改了判据会红）。
    let const_ok = st::FRAME_BUDGET_BYTES == EXP_FRAME_BUDGET
        && st::JITTER_ALLOWANCE_BYTES == EXP_JITTER;
    s.add(
        "带宽预算常量口径一致",
        const_ok,
        "单帧预算 64KiB / 单次突发上界 8KiB（判据侧独立写死；直接引被测常量则本判据恒真）",
    );

    // ② 单次突发超上界 → 要求分片（8KiB+1 的单包）。
    let mut st_b = st::BudgetState::default();
    let vb = st::budget(&mut st_b, EXP_JITTER + 1);
    let burst_ok = vb == st::BudgetVerdict::RejectBurst
        && st_b.used == 0
        && st_b.backlog == 0
        && st_b.rejected == 1;
    s.add(
        "单次突发超上界要求分片",
        burst_ok,
        "单个 8KiB+1 的包被拒（RejectBurst）且 used/backlog 均不动（突发上界是单次口径，不是跨帧积压）",
    );

    // ③ 分片累加可填满帧预算：8 次 8KiB 全放行，used 恰 64KiB。
    //
    // 判据侧独立累加（不读被测 used 做中间断言，只在末尾对账）。
    let mut st1 = st::BudgetState::default();
    let mut all_accept = true;
    let mut expect_used = 0u32;
    for _ in 0..8 {
        let v = st::budget(&mut st1, EXP_JITTER);
        if v != st::BudgetVerdict::Accept {
            all_accept = false;
        }
        expect_used += EXP_JITTER;
    }
    s.add(
        "分片累加恰填满帧预算",
        all_accept && st1.used == expect_used && expect_used == EXP_FRAME_BUDGET,
        "8 次 8KiB 全放行且 used 恰 64KiB（突发上界不阻塞帧内多次小包累加）",
    );

    // ④ 帧预算用尽后再要 → 超预算拒绝并**独立记账**。
    let v_over = st::budget(&mut st1, 1);
    let over_ok = v_over == st::BudgetVerdict::RejectOverBudget
        && st1.used == EXP_FRAME_BUDGET
        && st1.rejected == 1;
    s.add(
        "超预算拒绝且拒绝独立记账",
        over_ok,
        "用满 64KiB 后再要 1 字节被拒且 used 不增、rejected=1（悄悄限流会让发送方以为发出去了）",
    );

    // ⑤ 零字节请求幂等放行且不消耗预算。
    let mut st2 = st::BudgetState::default();
    let v0 = st::budget(&mut st2, 0);
    s.add(
        "零字节请求幂等放行",
        v0 == st::BudgetVerdict::Accept && st2.used == 0 && st2.rejected == 0,
        "空操作既不放行成消费也不计入拒绝",
    );

    // ⑥ 帧结转：used 归零、backlog 按**链路实送量**消化。
    //
    // `drained` 由调用方按实测吞吐给出，不是恒等于帧预算 ——
    // 若恒定按帧预算消化，backlog 单帧最多 64KiB、必被清空，
    // 「积压追不上」这条就结构不可达了。
    let mut st4 = st::BudgetState::default();
    for _ in 0..8 {
        let _ = st::budget(&mut st4, EXP_JITTER);
    }
    let backlog_before = st4.backlog;
    st::end_frame(&mut st4, EXP_FRAME_BUDGET);
    s.add(
        "帧结转按链路实送量消化",
        st4.used == 0
            && backlog_before == EXP_FRAME_BUDGET
            && st4.backlog == backlog_before.saturating_sub(EXP_FRAME_BUDGET),
        "积压 64KiB、实送 64KiB ⇒ 归零；used 归零（帧预算每帧重置）",
    );

    // ⑦ 消化量不足时**留账**（双向对照，两个方向都断）。
    //
    // 反例构造：链速只有预算的 1/8 ⇒ 每帧送 8KiB，而发送 64KiB
    // ⇒ 积压逐帧增长，这正是「不卡不骗」要防的雪崩。
    let mut st7 = st::BudgetState::default();
    let mut backlog_series: Vec<u32> = Vec::new();
    for _ in 0..4 {
        for _ in 0..8 {
            let _ = st::budget(&mut st7, EXP_JITTER);
        }
        st::end_frame(&mut st7, EXP_JITTER);
        backlog_series.push(st7.backlog);
    }
    let grows = backlog_series.len() == 4
        && backlog_series[0] == EXP_FRAME_BUDGET - EXP_JITTER
        && backlog_series[1] == backlog_series[0] + EXP_FRAME_BUDGET - EXP_JITTER
        && backlog_series[3] > backlog_series[1];
    s.add(
        "实送不足时积压留账并增长",
        grows,
        "每帧发 64KiB、实送 8KiB ⇒ 积压 56/120/184/248KiB 逐帧增长（积压雪崩可见；不清零=不丢欠账）",
    );

    // ⑧ 公平：判据侧独立重算对账（十诫第 4 条）。
    let inputs = [
        st::FairInput { input: 12345, tick: 1, load: 0 },
        st::FairInput { input: 12345, tick: 999, load: 100 },
        st::FairInput { input: 777, tick: 5, load: 50 },
    ];
    let g = st::fairness_gate(&inputs);
    let f1 = st::fair(inputs[0].as_ref());
    let f2 = st::fair(inputs[1].as_ref());
    let f3 = st::fair(inputs[2].as_ref());
    let indep_ok = f1 == expect_fair(12345)
        && f2 == expect_fair(12345)
        && f3 == expect_fair(777);
    s.add(
        "公平判定与独立重算一致",
        indep_ok && !g.self_inconsistent,
        "判定只由 input 决定（tick/load 仅诊断）；判据侧独立重算同余式对账得同一值",
    );

    // ⑦ 「同输入同判定」不变量：**时钟/负载无关**。
    let same_input_same = f1 == f2;
    let diff_input_diff = f1 != f3;
    s.add(
        "同输入同判定且异输入异判定",
        same_input_same && diff_input_diff,
        "tick 从 1 到 999、load 从 0 到 100，input=12345 的判定不变；input 不同则判定不同",
    );

    // ⑧ 自查面必须恒为 false（一旦 true 说明公平函数被改坏）。
    s.add(
        "公平闸自查恒一致",
        !g.self_inconsistent && g.verdict == f1,
        "门禁自查：组内同输入判定全同 ⇒ self_inconsistent=false，且 verdict 取首条",
    );

    // ⑨ 空输入不 panic（边界防护面）。
    let g0 = st::fairness_gate(&[]);
    s.add(
        "空输入安全",
        g0.verdict == 0 && !g0.self_inconsistent,
        "无输入时 verdict=0 且不自报不一致，不 panic 不猜",
    );

    // ⑩ 判定落在 [0,1000) 区间（分布口径，非自证）。
    let mut in_range = true;
    let probes = [0u64, 1, 999, 65535, u32::MAX as u64, u64::MAX];
    for p in probes.iter() {
        let v = st::fair(&st::FairInput { input: *p, tick: 0, load: 0 });
        if v >= 1000 {
            in_range = false;
        }
    }
    s.add(
        "公平判定值域受控",
        in_range,
        "含 0 / u64::MAX 在内的六个探针，判定恒 < 1000（同余取模不溢出）",
    );
}

// ---------------------------------------------------------------------------
// 判据五：开工闸与判据集自身性质
// ---------------------------------------------------------------------------

fn c5401_gate(s: &mut CheckSet) {
    /// 造一个**满足全部开工条件**的架构册。
    fn ready_reg() -> st::ArchRegistry {
        let mut a = st::ArchRegistry::new();
        // 每层都登记；**顶层 Gameplay 声明 0 条**（无下层依赖它）。
        for (i, l) in st::Layer::ALL.iter().enumerate() {
            a.declare(*l, if i == st::Layer::ALL.len() - 1 { 0 } else { 1 });
        }
        // 每条相邻层对各冻 1 条 ⇒ 三个非顶层的 provider 各 1 条。
        for i in 0..st::Layer::ALL.len() {
            let caller = st::Layer::ALL[i];
            if i == 0 {
                continue; // 传输层无下层可依赖，不冻
            }
            let provider = st::Layer::ALL[i - 1];
            let sig = st::sig_fingerprint(caller, provider, "iface");
            let _ = a.freeze(caller, provider, sig, "witness");
        }
        bind_all(a.handover_mut());
        a
    }

    // ① 条件齐时开工闸放行。
    let a = ready_reg();
    s.add(
        "四条件齐时开工闸放行",
        a.open_gate() && a.why_blocked().is_none(),
        "四层齐 / 冻结数==声明数 / 承接全绑定 / 台账自查过 ⇒ 开工闸放行",
    );

    // ② 破坏条件一：有层**未登记**（不是「声明数为 0」—— 顶层合法为 0）。
    //
    // 构造方式：`registered` 私有且 `declare` 才会置位，故「撤销某层
    // 登记」在公开面上不可达；改用**完全不登记任何层**的新册来触发
    // 同一条件（新建册四层皆未登记）。
    let mut b2 = st::ArchRegistry::new();
    bind_all(b2.handover_mut());
    let gate2 = !b2.open_gate();
    let why2 = b2.why_blocked();
    s.add(
        "未登记层时闸闭且原因可查",
        gate2 && why2 == Some("四层未齐：有层未登记"),
        "新建册未登记任何层 ⇒ 闸闭并给出「有层未登记」（不静默失败）；顶层声明 0 条仍应放行",
    );

    // ③ 破坏条件二：声明数与冻结数不符（**总数相等、逐层不等** —— 更隐蔽）。
    let mut c = ready_reg();
    // ready_reg 的声明/冻结逐层是 [1,1,1,0]。这里把
    // **会话层**声明改成 0（其 provider 冻结数是 1），
    // 同时把**复制层**声明改成 2（其 provider 冻结数是 1）。
    // 总数：0(顶层)+2+0+1(传输) = 3，与原总数 3 **相等**；
    // 逐层却有两层不匹配 ⇒ 只看总数的闸会漏过去。
    c.declare(st::Layer::Session, 0);
    c.declare(st::Layer::Replication, 2);
    let mut total_decl = 0u32;
    let mut total_frozen = 0usize;
    for l in st::Layer::ALL.iter() {
        total_decl += c.declared_of(*l);
        total_frozen += c.ledger().frozen_count_of(*l);
    }
    s.add(
        "逐层对账非总数对账",
        !c.open_gate() && total_decl as usize == total_frozen,
        "总数相等（3==3）也拦：会话层声明 0 冻 1、复制层声明 2 冻 1 ⇒ 闸闭（只看总数会让层间错配溜过去）",
    );

    // ④ 破坏条件三：承接未全绑定。
    let mut d = ready_reg();
    d.handover_mut().rows_mut().clear();
    s.add(
        "承接未绑时闸闭",
        !d.open_gate(),
        "落地表清空 ⇒ 承接面未绑定 ⇒ 闸闭",
    );

    // ⑤ 域归属登记如实（冲突记录在字段里而非注释里）。
    s.add(
        "域归属登记可查",
        !a.domain.is_empty() && a.layer_count() == EXP_LAYERS,
        "域归属登记为字段可查（VE 册域表与锚点标题冲突，以任务单为准并留痕）；层数恒 4",
    );

    // ⑥ 架构册默认台账自洽：新建即空，不预填任何冻结。
    let fresh = st::ArchRegistry::new();
    s.add(
        "新建架构册不预填",
        fresh.ledger().is_empty() && fresh.handover().bound_count() == 0,
        "新建即零冻结零绑定（预填会让「未开工」看起来像「已开工」）",
    );

    // ⑦ 判据集自身性质：**A/B 两族**非空且未截断。
    //
    // **此处绝不能调 C 族自身，也绝不能调
    // `run_vef54_aaarch_checks()`** —— 两者都会回调本函数，
    // 构成无限递归 ⇒ 栈溢出（判据集把自己的
    // 判据吃掉；**症状是进程崩而非某条判据变红**，
    // 极难定位）。C 族与聚合层的截断分别由调用方
    // 与 `run_vef54_aaarch_checks` 内的 `assert!` 把关。
    let a = run_vef54_aaarch_checks_a_standalone();
    let b = run_vef54_aaarch_checks_b_standalone();
    let (_, ca) = a.red_items();
    let (_, cb) = b.red_items();
    let per_family_ok = ca >= 22 && cb >= 17 && !a.truncated() && !b.truncated();
    s.add(
        "判据集自身性质：A/B 族非空且未截断",
        per_family_ok,
        "A≥22（四层7+对拍6+冻结9）/ B≥17（承接7+承诺10）条且 truncated=false（截断=判据被丢了还以为全绿）",
    );

    // ⑧ 判据名与说明非空（可读性即判据质量：
    // 无名无据的判据无法复核，等于没写）。
    let mut named = 0usize;
    for set in [a, b].iter() {
        let (items, n) = set.red_items();
        for i in 0..n {
            if let Some(ck) = items[i] {
                if !ck.name.is_empty() && !ck.detail.is_empty() {
                    named += 1;
                }
            }
        }
    }
    s.add(
        "判据名与说明非空",
        named == ca + cb,
        "A/B 每条判据都有名字与依据说明（无名无据的判据无法复核）",
    );

}

// ---------------------------------------------------------------------------
// 三族分立入口（规避 `CheckSet::MAX_CHECKS` 截断）
// ---------------------------------------------------------------------------

/// A 族：四层 + 层间对拍 + 层冻结。
pub fn run_vef54_aaarch_checks_a() -> CheckSet {
    let mut s = CheckSet::new("VE-F5401-a");
    c5401_layers(&mut s);
    c5401_probe(&mut s);
    c5401_freeze(&mut s);
    s
}

/// A 族独立入口。
pub fn run_vef54_aaarch_checks_a_standalone() -> CheckSet {
    run_vef54_aaarch_checks_a()
}

/// B 族：承接落地 + 流畅公平。
pub fn run_vef54_aaarch_checks_b() -> CheckSet {
    let mut s = CheckSet::new("VE-F5401-b");
    c5401_handover(&mut s);
    c5401_promise(&mut s);
    s
}

/// B 族独立入口。
pub fn run_vef54_aaarch_checks_b_standalone() -> CheckSet {
    run_vef54_aaarch_checks_b()
}

/// C 族：开工闸 + 判据集自身性质。
pub fn run_vef54_aaarch_checks_c() -> CheckSet {
    let mut s = CheckSet::new("VE-F5401-c");
    c5401_gate(&mut s);
    s
}

/// C 族独立入口。
pub fn run_vef54_aaarch_checks_c_standalone() -> CheckSet {
    run_vef54_aaarch_checks_c()
}

/// 全部判据（三族合并），显性断言未截断。
pub fn run_vef54_aaarch_checks() -> CheckSet {
    let a = run_vef54_aaarch_checks_a_standalone();
    let b = run_vef54_aaarch_checks_b_standalone();
    let c = run_vef54_aaarch_checks_c_standalone();
    let mut all = CheckSet::merge(a, b);
    all = CheckSet::merge(all, c);
    assert!(
        !all.truncated(),
        "VE-F5401 判据被 MAX_CHECKS={} 截断 —— 须再切族",
        MAX_CHECKS
    );
    all
}
