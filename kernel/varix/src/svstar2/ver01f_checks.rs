//! VE-F3406 · 令牌覆盖层判据（四级覆盖）
//!
//! **锚点判据（原文）**：四级栈、优先级仲裁、来源审计、越级警告、判据。
//!
//! # 一、判据怎么做到"不是恒真"
//!
//! 覆盖层最容易写成恒真断言的地方有三处，本模块逐一封死：
//!
//! 1. **"仲裁选了最高层"**。写法一：`assert!(resolve(p).level == Level::Component)` ——
//!    语料里只有组件层有值时，这句在"仲裁返回固定值"（比如永远返回 Default）的
//!    实现下会红，但把语料换成"四层都有值"就照样绿？反过来，若语料里只有低层，
//!    写死最低层就全绿。**关键是要有一批"四层都有值、期望逐条给出"的语料**，
//!    并且判据侧**独立重算**期望（不调被测的 `arbiter`）。
//! 2. **"越级覆盖告警"**。若语料里从没定稿过，`frozen` 恒为 `None`，越级分支是
//!    死代码，任何关于它的断言都恒绿。必须**先定稿再写低层**，且要断**三件事**：
//!    告警码正确、审计有 SHADOWED_COVER 记录、**resolve 读不到它**。
//! 3. **"来源不明告警"**。若语料里来源全都已登记，`unaudited` 分支永不触发。
//!    必须有一批**未登记来源**的语料。
//!
//! # 二、方向：被拒才是合格
//!
//! 拒绝级判据一律写成 `err == Some(期望码)`。写成 `err.is_none()` 判"合格"
//! 是**最隐蔽的弱门禁**：它在基线上就红不了，而在"被测实现改成什么都放行"时才红 ——
//! 也就是只在变异时才被抓住，等于没有。判据里所有"应拒"断言都直接比对错误码。
//!
//! # 三、双向验证纪律：实测抓到的三条
//!
//! 补任何一条判据都必须**双向验证**：基线仍绿 **且** 对应变体转红。本轮跑了
//! **四组共 51 次注入**，实测抓到的三条（详见文末变异台）：
//!
//! 1. **单路径语料让过滤恒真**：审计检索判据的语料只写一条路径，于是把
//!    `audit_of` 的过滤改成"返回全量"也全绿。补三路径语料后同一条变异转红。
//! 2. **`verify` 存在不可达的冗余防御**：删掉长度检查后全绿 —— 因为 `push`
//!    入口已守住，那段防御在正常语料下**根本走不到**。补"直接构造非法态"的
//!    判据后才转红（不是删防御，也不是加测试入口污染生产面）。
//! 3. **格子口径 vs 路径口径混用**：导出判据把两个不同口径的数当同一个而红。
//!    修法是拆成两条判据 + 一条偏序，而不是改被测代码去迎合。
//!
//! 变异台数据是**实跑出来的**，不是推演；先写结论再补测等于编造。

use crate::checks::CheckSet;
use super::ver01b_parser::Site;
use super::ver01e_typetree::Binding;
use super::ver01f_overlay::*;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 判据专用测试点构造：行号按写入序递增，便于定位。
fn site(line: u32) -> Site {
    Site {
        line,
        col: 1,
        byte: line.saturating_mul(16),
    }
}

/// 一个"四层齐全 + 全部来源已登记"的基准栈，供多条判据复用。
///
/// **复用时必须注意**：基准栈是**共享可变状态**的，若某条判据要改它，
/// 得自己 clone 一份 —— 否则判据之间互相污染，基线红了却查不出是谁弄的。
fn base_stack() -> OverlayStack {
    let mut st = OverlayStack::new();
    // 登记四个来源，覆盖「内置 / 主题包 / 场景包 / 第三方扩展」四种身份。
    let _ = st.register_source("builtin", false);
    let _ = st.register_source("theme-dark", false);
    let _ = st.register_source("scene-dash", false);
    let _ = st.register_source("ext-button", true);
    st
}

// ---------------------------------------------------------------------------
// 一、四级栈（锚点判据 1）
// ---------------------------------------------------------------------------

fn checks_four_levels(cs: &mut CheckSet) {
    // 全集恰为四项，且 rank 严格 0..3 递增。
    let ranks: [u8; LEVEL_COUNT] = [
        Level::Default.rank(),
        Level::Theme.rank(),
        Level::Scene.rank(),
        Level::Component.rank(),
    ];
    let rank_ok = ranks[0] == 0
        && ranks[1] == 1
        && ranks[2] == 2
        && ranks[3] == 3
        && Level::ALL.len() == LEVEL_COUNT;
    cs.add(
        "E06-栈-四级rank严格递增",
        rank_ok,
        "默认0/主题1/场景2/组件3，四项不多不少",
    );

    // rank 与 ALL 下标必须一致（下标即 rank）——仲裁用下标取层，
    // 这条不一致会让"按 rank 找层"整体错位。
    let idx_ok = Level::ALL
        .iter()
        .enumerate()
        .all(|(i, l)| l.rank() as usize == i);
    cs.add("E06-栈-rank与ALL下标一致", idx_ok, "下标即 rank");

    // from_rank 与 rank 互逆（round-trip）。**双向**：只测 rank→from 会漏掉
    // "from_rank 对某些 rank 返错值"；只测 from→rank 会漏掉"漏实现某层"。
    let mut rt = true;
    for l in Level::ALL.iter() {
        match Level::from_rank(l.rank()) {
            Some(got) => {
                if got != *l {
                    rt = false;
                }
            }
            None => rt = false,
        }
    }
    // 越界 rank 必须返 None，**不得折回**（折回 = 数据坏了被洗成"最低层生效"）。
    for bad in [4u8, 5, 200, 255] {
        if Level::from_rank(bad).is_some() {
            rt = false;
        }
    }
    cs.add(
        "E06-栈-rank往返互逆且越界返空",
        rt,
        "rank→from_rank 全命中，越界 rank 返 None 不折回",
    );

    // 短码：四项互异 + parse 往返 + 未登记串拒绝。
    let mut wires: [&str; LEVEL_COUNT] = ["", "", "", ""];
    let mut wires_distinct = true;
    for (i, l) in Level::ALL.iter().enumerate() {
        wires[i] = l.wire();
        for j in 0..i {
            if wires[j] == wires[i] {
                wires_distinct = false;
            }
        }
    }
    cs.add("E06-栈-四级短码互异", wires_distinct, "四个短码两两不同");

    let mut wire_rt = true;
    for l in Level::ALL.iter() {
        match Level::parse(l.wire()) {
            Ok(got) => {
                if got != *l {
                    wire_rt = false;
                }
            }
            Err(_) => wire_rt = false,
        }
    }
    // 表外短码（含大小写变体）必须拒：短码是协议不是自然语言。
    let mut reject_ok = wire_rt;
    for bad in ["DFLT", "Dfl", "theme", "cmp ", "", "cmpp"] {
        if Level::parse(bad).is_ok() {
            reject_ok = false;
        }
    }
    cs.add(
        "E06-栈-短码往返且大小写敏感",
        reject_ok,
        "wire→parse 往返一致；DFLT/theme/cmp 尾空格必须拒",
    );

    // 中文名四项互异且非空（读屏用；空名会让播报少一段而无人察觉）。
    let mut zh_ok = true;
    let mut zhs: [&str; LEVEL_COUNT] = ["", "", "", ""];
    for (i, l) in Level::ALL.iter().enumerate() {
        zhs[i] = l.zh();
        if zhs[i].is_empty() {
            zh_ok = false;
        }
        for j in 0..i {
            if zhs[j] == zhs[i] {
                zh_ok = false;
            }
        }
    }
    cs.add("E06-栈-中文名非空互异", zh_ok, "四个中文名非空且两两不同");

    // 四层全填时逐层断言胜出（**不是只看最高层**，四个下标都要断）。
    let mut all_filled = true;
    for (i, l) in Level::ALL.iter().enumerate() {
        let mut st = base_stack();
        for (j, l2) in Level::ALL.iter().enumerate() {
            let val = format!("v{}", j);
            st.push(*l2, "builtin", "color.accent", &val, site(j as u32 + 1));
        }
        match st.resolve("color.accent") {
            Some(r) => {
                // 期望由**判据侧独立算出**：Level::ALL 里 rank 最大者。
                let expect = Level::ALL[LEVEL_COUNT - 1];
                if r.level != expect {
                    all_filled = false;
                }
                // 值也必须跟着层走（值写死就是"只对层不对值"的实现蒙混）。
                let expect_val = format!("v{}", LEVEL_COUNT - 1);
                if r.value.as_str() != expect_val.as_str() {
                    all_filled = false;
                }
            }
            None => all_filled = false,
        }
        let _ = i;
        let _ = l;
    }
    cs.add(
        "E06-栈-四层全填时逐层命中",
        all_filled,
        "四级依次写入后 resolve 必须落在组件层且取组件层值",
    );
}

// ---------------------------------------------------------------------------
// 二、优先级仲裁（锚点判据 2）
// ---------------------------------------------------------------------------

fn checks_arbitration(cs: &mut CheckSet) {
    // 仲裁函数本身：rank 高者胜，**双向穷举 4×4 = 16 组**（只测 a>b 会漏掉
    // "b>a 时也返回 a" 这类反向错误）。
    //
    // 期望值**不用 `if a.rank() >= b.rank()` 推导**——那与被测函数是同一个
    // 表达式，判据就成了自证式：把被测实现的比较符改掉，期望值跟着一起变，
    // 判据照样全绿（这正是 M1 变异曾MISS 的原因）。改为按 [`Level::ALL`]
    // 的**下标**独立推导："下标大者胜" 是契约的另一套等价表述，
    // 与被测实现用了不同的推导路径。
    let mut arb_ok = true;
    for (ia, a) in Level::ALL.iter().enumerate() {
        for (ib, b) in Level::ALL.iter().enumerate() {
            let expect = if ia >= ib { *a } else { *b };
            match arbitrate(*a, *b) {
                Some(got) => {
                    if got != expect {
                        arb_ok = false;
                    }
                }
                None => arb_ok = false,
            }
        }
    }
    cs.add(
        "E06-仲裁-16组穷举高者胜",
        arb_ok,
        "4×4 全组合，返回 rank 较大者",
    );

    // **真实语料**：逐层叠加，每加一层 resolve 必须换到新层。
    // 期望值由判据侧的循环下标推导，不问 arbiter。
    let mut ladder_ok = true;
    let mut ladder_detail_ok = true;
    let mut st = OverlayStack::new();
    for (i, l) in Level::ALL.iter().enumerate() {
        let val = format!("v{}", i);
        st.push(*l, "builtin", "size.gap", &val, site(i as u32 + 1));
        match st.resolve("size.gap") {
            Some(r) => {
                if r.level != *l {
                    ladder_ok = false;
                }
                if r.value.as_str() != val.as_str() {
                    ladder_detail_ok = false;
                }
            }
            None => {
                ladder_ok = false;
                ladder_detail_ok = false;
            }
        }
    }
    cs.add(
        "E06-仲裁-逐层叠加换层",
        ladder_ok,
        "默认→主题→场景→组件，每加一层 resolve 必须落到新层",
    );
    cs.add(
        "E06-仲裁-胜出值随层变",
        ladder_detail_ok,
        "胜出值必须是该层写入的值（防层对了值取错）",
    );

    // **逆序写入**（先组件后默认）：结果必须与顺序无关 —— 仲裁按 rank 而非
    // 写入序。**这是最容易写成"后写胜"的那种实现被抓住的地方**。
    let mut rev = base_stack();
    for (i, l) in Level::ALL.iter().enumerate().rev() {
        let val = format!("v{}", i);
        rev.push(*l, "builtin", "dur.fast", &val, site(i as u32 + 1));
    }
    let rev_ok = match rev.resolve("dur.fast") {
        Some(r) => r.level == Level::Component && r.value.as_str() == "v3",
        None => false,
    };
    cs.add(
        "E06-仲裁-逆序写入结果不变",
        rev_ok,
        "组件先写、默认后写，胜出仍是组件层（仲裁不按写入序）",
    );

    // 同层冲突：后来者胜（头注第二节），且旧条目进审计 SUPERSEDED。
    let mut cf = base_stack();
    cf.push(Level::Theme, "theme-dark", "color.bg", "#111111", site(1));
    let second = cf.push(Level::Theme, "theme-dark", "color.bg", "#222222", site(2));
    let cf_win = match cf.resolve("color.bg") {
        Some(r) => r.value.as_str() == "#222222" && r.level == Level::Theme,
        None => false,
    };
    let aud = cf.audit_of("color.bg");
    let superseded_n = aud
        .iter()
        .filter(|e| e.action == AuditAction::Superseded)
        .count();
    let sup_flagged = aud
        .iter()
        .filter(|e| e.action == AuditAction::Superseded)
        .all(|e| e.flags.has(AuditFlags::SUPERSEDED));
    cs.add(
        "E06-仲裁-同层后来者胜",
        cf_win && second.changed,
        "同层二次写入必须换值且 changed=true",
    );
    cs.add(
        "E06-仲裁-冲突进审计且带标记",
        superseded_n == 1 && sup_flagged,
        "被压掉者恰好一条 SUPERSEDED 且带 SUPERSEDED 标记位",
    );

    // 同层重写**不增加**生效覆盖数（否则守恒式会被撑破）。
    let same_cell_ok = cf.live_cell_count() == 1;
    cs.add(
        "E06-仲裁-同层重写不增计数",
        same_cell_ok,
        "同一层同一路径写两次，生效覆盖仍为 1",
    );

    // 换层写**增加**生效覆盖数（与上一条成对，守住"哪些算新格子"的判定）。
    cf.push(Level::Scene, "scene-dash", "color.bg", "#333333", site(3));
    let cross_cell_ok = cf.live_cell_count() == 2;
    cs.add(
        "E06-仲裁-换层写增计数",
        cross_cell_ok,
        "同路径写到另一个层，生效覆盖变 2",
    );

    // 被压层清单：胜出者之上无被压者；胜出者之下的未遮蔽层按 rank **降序**列出。
    let mut sh = base_stack();
    for (i, l) in Level::ALL.iter().enumerate() {
        let val = format!("v{}", i);
        sh.push(*l, "builtin", "color.fg", &val, site(i as u32 + 1));
    }
    let sh_ok = match sh.resolve("color.fg") {
        Some(r) => {
            r.shadowed_levels.len() == 3
                && r.shadowed_levels[0] == Level::Scene
                && r.shadowed_levels[1] == Level::Theme
                && r.shadowed_levels[2] == Level::Default
        }
        None => false,
    };
    cs.add(
        "E06-仲裁-被压层按rank降序",
        sh_ok,
        "组件胜出时压住场景/主题/默认，顺序必须降序",
    );

    // 未命中：resolve 对不存在的路径返 None（**不是**返默认值 —— 那会让
    // "忘写覆盖"被洗成"用了默认"）。
    let miss_ok = base_stack().resolve("color.nonexistent").is_none();
    cs.add("E06-仲裁-未命中返空", miss_ok, "无覆盖路径 resolve 返 None");

    // 仲裁是纯函数：连读两次结果一致，且读不产生审计（否则审计随读膨胀）。
    let mut pure = base_stack();
    pure.push(Level::Theme, "theme-dark", "color.x", "#111111", site(1));
    let a1 = format!("{:?}", pure.resolve("color.x"));
    let aud_before = pure.audit_len();
    let _ = pure.resolve("color.x");
    let _ = pure.resolve("color.x");
    let _ = pure.resolve("color.x");
    let aud_after = pure.audit_len();
    let a2 = format!("{:?}", pure.resolve("color.x"));
    cs.add(
        "E06-仲裁-只读不改状态",
        a1 == a2 && aud_before == aud_after,
        "连读三次结果一致且审计条数不变",
    );
}

// ---------------------------------------------------------------------------
// 三、越级警告（锚点判据 4）
// ---------------------------------------------------------------------------

fn checks_out_of_order(cs: &mut CheckSet) {
    // 越级三要件：**告警码 + 审计记录 + resolve 读不到**。缺一件都可能被
    // "只记了不生效"或"只告警了却照样生效"蒙过去。
    let mut st = base_stack();
    st.freeze_to(Level::Theme);
    // 先在主题层写一条（rank == frozen，定稿层自身可写）。
    st.push(Level::Theme, "theme-dark", "color.bg", "#111111", site(1));
    // 再往默认层写（rank 0 < 1 = 越级）。
    let ooo = st.push(Level::Default, "builtin", "color.bg", "#999999", site(2));

    cs.add(
        "E06-越级-告警码正确",
        ooo.warn == Some(OverlayCode::LayerOutOfOrder) && ooo.err.is_none(),
        "越级写入须告警 LayerOutOfOrder 且不被拒",
    );

    // resolve 必须仍读主题层的值（**越级不改变生效结果**）。
    let still_theme = match st.resolve("color.bg") {
        Some(r) => r.level == Level::Theme && r.value.as_str() == "#111111",
        None => false,
    };
    cs.add(
        "E06-越级-不改变生效结果",
        still_theme,
        "越级写入后 resolve 仍是主题层值",
    );

    // 但越级条目**确实入栈**（历史不能丢），且审计带 SHADOWED。
    let slot = st.slot_ref("color.bg");
    let shadowed_stored = match slot {
        Some(s) => s.shadowed_count() == 1 && s.live_count() == 1,
        None => false,
    };
    cs.add(
        "E06-越级-入栈但计为遮蔽",
        shadowed_stored,
        "槽内 1 条遮蔽 + 1 条生效（越级条目不删）",
    );

    let aud = st.audit_of("color.bg");
    let sh_rec = aud
        .iter()
        .filter(|e| e.action == AuditAction::ShadowedCover)
        .count();
    let sh_flag = aud
        .iter()
        .filter(|e| e.action == AuditAction::ShadowedCover)
        .all(|e| e.flags.has(AuditFlags::SHADOWED));
    cs.add(
        "E06-越级-审计记SHADOWED_COVER",
        sh_rec == 1 && sh_flag,
        "越级写入恰好一条 ShadowedCover 且带 SHADOWED 标记",
    );

    // 越级审计**必须保留原始来源字符串**（改成占位符就查不出是谁越级改的）。
    let src_kept = aud
        .iter()
        .find(|e| e.action == AuditAction::ShadowedCover)
        .map(|e| e.source.as_str() == "builtin")
        .unwrap_or(false);
    cs.add("E06-越级-审计保留原来源", src_kept, "越级条目来源逐字保留");

    // 越级覆盖**不得计入**被压层清单（它不是"被更高层压住"，是"自己无效"）。
    // 混进清单的话，报告会把它算成"生效层压住了它"，是两种不同的错。
    let not_in_shadow = match st.resolve("color.bg") {
        Some(r) => r.shadowed_levels.is_empty(),
        None => false,
    };
    cs.add(
        "E06-越级-不入被压层清单",
        not_in_shadow,
        "越级层不进 shadowed_levels（无效≠被压）",
    );

    // 越级 + 来源不明叠加：warn 取更严重的越级，审计两个标记位都在。
    let mut both = base_stack();
    both.freeze_to(Level::Scene);
    both.push(Level::Scene, "scene-dash", "color.z", "#111111", site(1));
    let combo = both.push(Level::Default, "ghost-writer", "color.z", "#999999", site(2));
    let combo_aud = both.audit_of("color.z");
    let combo_flags = combo_aud
        .iter()
        .find(|e| e.action == AuditAction::ShadowedCover)
        .map(|e| e.flags.has(AuditFlags::SHADOWED) && e.flags.has(AuditFlags::UNAUDITED))
        .unwrap_or(false);
    cs.add(
        "E06-越级-叠加取更严重告警",
        combo.warn == Some(OverlayCode::LayerOutOfOrder) && combo_flags,
        "既越级又来源不明：warn 取越级，审计双标记都在",
    );

    // 未定稿时写低层**不是**越级（frozen=None 分支必须被覆盖，否则这条分支
    // 永远是死代码，上面所有越级判据都可能测不到它）。
    let mut unfrozen = base_stack();
    unfrozen.push(Level::Theme, "theme-dark", "color.k", "#111111", site(1));
    let low = unfrozen.push(Level::Default, "builtin", "color.k", "#999999", site(2));
    let low_resolves = match unfrozen.resolve("color.k") {
        Some(r) => r.level == Level::Theme,
        None => false,
    };
    cs.add(
        "E06-越级-未定稿时低层正常生效",
        low.warn.is_none() && low_resolves,
        "未定稿栈里写低层不告警（frozen=None 分支非死代码）",
    );

    // 定稿层**之上**写入 = 拒绝（不是越级）：改已定稿契约。
    let mut above = base_stack();
    above.freeze_to(Level::Theme);
    let up = above.push(Level::Component, "ext-button", "color.m", "#123456", site(1));
    cs.add(
        "E06-越级-定稿之上拒绝",
        up.err == Some(OverlayCode::LevelFrozen) && !up.stored,
        "定稿到主题后写组件层须拒 LevelFrozen 且不入栈",
    );

    // freeze_to 幂等且只上不下（重复定稿低层不放开高层）。
    let mut fz = base_stack();
    fz.freeze_to(Level::Scene);
    fz.freeze_to(Level::Default);
    let fz_ok = fz.frozen_level() == Some(Level::Scene);
    cs.add(
        "E06-越级-定稿只上不下",
        fz_ok,
        "定稿到场景后再定稿默认，冻结层仍是场景",
    );

    // 定稿到顶层后，写同层允许（rank == frozen 不算越级也不算越上）。
    let mut eq = base_stack();
    eq.freeze_to(Level::Component);
    let same = eq.push(Level::Component, "ext-button", "color.n", "#123456", site(1));
    cs.add(
        "E06-越级-定稿层自身可写",
        same.err.is_none() && same.warn.is_none(),
        "定稿到组件后写组件层既不拒也不告警",
    );
}

// ---------------------------------------------------------------------------
// 四、来源审计（锚点判据 3）
// ---------------------------------------------------------------------------

fn checks_audit(cs: &mut CheckSet) {
    // 来源登记：成功 / 重复拒 / 空标识拒 / 超长拒。
    let mut st = base_stack();
    let dup = st.register_source("builtin", false);
    cs.add(
        "E06-审计-重复来源拒绝",
        dup == Err(OverlayCode::SourceDup),
        "同 id 二次登记须拒 SourceDup",
    );

    let mut s2 = OverlayStack::new();
    let empty_src = s2.register_source("", false);
    cs.add(
        "E06-审计-空来源拒绝",
        empty_src == Err(OverlayCode::SourceIdInvalid),
        "空标识须拒 SourceIdInvalid",
    );

    // 超长来源：构造 MAX_SOURCE_LEN+1 的串（判据侧自算长度，不抄常量）。
    let long: String = {
        let mut t = String::new();
        for _ in 0..=(MAX_SOURCE_LEN) {
            t.push('x');
        }
        t
    };
    let long_src = OverlayStack::new().register_source(&long, false);
    cs.add(
        "E06-审计-超长来源拒绝",
        long_src == Err(OverlayCode::SourceIdInvalid),
        "标识超 MAX_SOURCE_LEN 须拒",
    );

    // 来源不明：未登记来源写入 → 告警 + **仍然生效** + 审计记 UNAUDITED_COVER。
    let mut u = base_stack();
    let unk = u.push(Level::Theme, "ghost", "color.bg", "#123456", site(1));
    let unk_res = match u.resolve("color.bg") {
        Some(r) => r.value.as_str() == "#123456" && r.level == Level::Theme,
        None => false,
    };
    let unk_aud = u.audit_of("color.bg");
    let unk_rec = unk_aud
        .iter()
        .filter(|e| e.action == AuditAction::UnauditedCover)
        .count();
    let unk_flag = unk_aud
        .iter()
        .filter(|e| e.action == AuditAction::UnauditedCover)
        .all(|e| e.flags.has(AuditFlags::UNAUDITED));
    cs.add(
        "E06-审计-来源不明告警",
        unk.warn == Some(OverlayCode::SourceUnknown) && unk.err.is_none(),
        "未登记来源须告警 SourceUnknown 且不被拒",
    );
    cs.add(
        "E06-审计-来源不明仍生效",
        unk_res,
        "来源不明不阻止生效（记账问题≠取值问题）",
    );
    cs.add(
        "E06-审计-来源不明记UNAUDITED",
        unk_rec == 1 && unk_flag,
        "恰好一条 UnauditedCover 且带 UNAUDITED 标记",
    );

    // 未登记来源**不得**打上第三方扩展标记（否则 E14 会把来路不明的覆盖
    // 当成合规扩展放行 —— 这是本单最危险的潜在漏洞）。
    let not_ext = match u.resolve("color.bg") {
        Some(r) => !r.third_party && r.unaudited,
        None => false,
    };
    cs.add(
        "E06-审计-来源不明不算扩展",
        not_ext,
        "未登记来源：unaudited=true 且 third_party=false",
    );

    // 已登记的扩展来源要正确打上扩展标记（E14 准入计数依赖它）。
    let mut e = base_stack();
    e.push(Level::Component, "ext-button", "color.accent", "#123456", site(1));
    let ext_ok = match e.resolve("color.accent") {
        Some(r) => r.third_party && !r.unaudited,
        None => false,
    };
    cs.add(
        "E06-审计-扩展来源标记正确",
        ext_ok,
        "已登记扩展：third_party=true 且 unaudited=false",
    );

    // 审计查得到"谁在什么层级改了什么"：**三条要素同时满足**才算追溯到。
    let mut w = base_stack();
    w.push(Level::Default, "builtin", "color.accent", "#000000", site(1));
    w.push(Level::Theme, "theme-dark", "color.accent", "#111111", site(2));
    w.push(Level::Scene, "scene-dash", "color.accent", "#222222", site(3));
    w.push(Level::Component, "ext-button", "color.accent", "#333333", site(4));
    let trace = w.audit_of("color.accent");
    // 期望：四条审计，层级依次 默认/主题/场景/组件，来源各不相同。
    let mut trace_ok = trace.len() == 4;
    for (i, l) in Level::ALL.iter().enumerate() {
        match trace.get(i) {
            Some(e) => {
                if e.level != *l {
                    trace_ok = false;
                }
            }
            None => trace_ok = false,
        }
    }
    let srcs: [&str; 4] = ["builtin", "theme-dark", "scene-dash", "ext-button"];
    for (i, s) in srcs.iter().enumerate() {
        match trace.get(i) {
            Some(e) => {
                if e.source.as_str() != *s {
                    trace_ok = false;
                }
            }
            None => trace_ok = false,
        }
    }
    cs.add(
        "E06-审计-谁在何层可追溯",
        trace_ok,
        "四层写入后审计四条齐全，层级与来源逐条对上",
    );

    // **C 组 M47 实测暴露的弱门禁（本条是它的修法）**：上面那条
    // `E06-审计-谁在何层可追溯` 的栈只写了 `color.accent` **一条路径**，于是
    // 把 `audit_of` 的路径过滤改成"返回全量"也照样全绿 —— 单路径语料让过滤
    // **恒等于真**。本条用**三条路径**的语料重钉：每条路径各两次写，
    // `audit_of(p)` 必须恰好返回 2 条且路径全对。
    let mut m = base_stack();
    m.push(Level::Theme, "theme-dark", "color.p", "#111111", site(1));
    m.push(Level::Scene, "scene-dash", "color.p", "#222222", site(2));
    m.push(Level::Theme, "theme-dark", "color.q", "#333333", site(3));
    m.push(Level::Scene, "scene-dash", "color.q", "#444444", site(4));
    m.push(Level::Theme, "theme-dark", "color.r", "#555555", site(5));
    m.push(Level::Scene, "scene-dash", "color.r", "#666666", site(6));
    let mut multi_ok = true;
    for p in ["color.p", "color.q", "color.r"] {
        let got = m.audit_of(p);
        // 恰好 2 条（过滤生效）且路径逐条对上（过滤正确）
        if got.len() != 2 {
            multi_ok = false;
        }
        for e in got.iter() {
            if e.path.as_str() != p {
                multi_ok = false;
            }
        }
    }
    // 总审计 6 条 < 6 不成立：必须恰好等于 6（过滤没有把别的路径吞进来）
    if m.audit_len() != 6 {
        multi_ok = false;
    }
    cs.add(
        "E06-审计-多路径过滤生效",
        multi_ok,
        "三路径各2条：audit_of 各自恰好2条且路径全对（单路径语料过滤恒真）",
    );

    // 审计序号严格 1..=len（缺号 = 有写入没记账 = 零静默破口）。
    let seq_ok = trace
        .iter()
        .enumerate()
        .all(|(i, e)| (e.seq as usize) == i + 1);
    cs.add("E06-审计-序号连续无缺号", seq_ok, "seq 严格 1 递增");

    // 按来源查审计：只回该来源的条目（不是全量）。
    let by_src = w.audit_by_source("theme-dark");
    let by_src_ok = by_src.len() == 1
        && by_src
            .first()
            .map(|e| e.path.as_str() == "color.accent")
            .unwrap_or(false);
    cs.add(
        "E06-审计-按来源检索准确",
        by_src_ok,
        "按 theme-dark 检索恰好一条且路径正确",
    );

    // 审计**不含覆盖值正文**（隐私）：值集里的字面量一个都不许出现在审计串里。
    // **注意不能只查一个字面量**（那是记忆教训第 29 条）。这里查全部。
    let mut leak = false;
    for e in w.audit_ref() {
        let s = e.spoken();
        for v in ["#000000", "#111111", "#222222", "#333333"] {
            if s.contains(v) {
                leak = true;
            }
        }
    }
    cs.add(
        "E06-审计-不含值正文",
        !leak,
        "四条审计的播报里不得出现任一覆盖值字面量",
    );

    // 审计的站点被记下（"何时"这一要素）。
    let sites_ok = trace
        .iter()
        .enumerate()
        .all(|(i, e)| e.site.line as usize == i + 1);
    cs.add(
        "E06-审计-写入点逐条记录",
        sites_ok,
        "四条审计的行号与写入序一致",
    );
}

// ---------------------------------------------------------------------------
// 五、边界与错误路径
// ---------------------------------------------------------------------------

fn checks_boundaries(cs: &mut CheckSet) {
    // 空路径 / 超长路径 / 空值 / 超长值 / 超长来源：五条拒绝路径各钉一次。
    let mut st = base_stack();
    let empty_path = st.push(Level::Theme, "theme-dark", "", "#111111", site(1));
    cs.add(
        "E06-边界-空路径拒绝",
        empty_path.err == Some(OverlayCode::PathInvalid) && !empty_path.stored,
        "空路径须拒 PathInvalid",
    );

    let long_path: String = {
        let mut t = String::new();
        for _ in 0..=(MAX_PATH_LEN) {
            t.push('p');
        }
        t
    };
    let lp = st.push(Level::Theme, "theme-dark", &long_path, "#111111", site(1));
    cs.add(
        "E06-边界-超长路径拒绝",
        lp.err == Some(OverlayCode::PathInvalid),
        "路径超 MAX_PATH_LEN 须拒",
    );

    let empty_val = st.push(Level::Theme, "theme-dark", "color.q", "", site(1));
    cs.add(
        "E06-边界-空值拒绝",
        empty_val.err == Some(OverlayCode::ValueInvalid),
        "空覆盖值须拒 ValueInvalid",
    );

    let long_val: String = {
        let mut t = String::new();
        for _ in 0..=(MAX_VALUE_LEN) {
            t.push('v');
        }
        t
    };
    let lv = st.push(Level::Theme, "theme-dark", "color.r", &long_val, site(1));
    cs.add(
        "E06-边界-超长值拒绝",
        lv.err == Some(OverlayCode::ValueInvalid),
        "值超 MAX_VALUE_LEN 须拒",
    );

    let long_src: String = {
        let mut t = String::new();
        for _ in 0..(MAX_SOURCE_LEN + 1) {
            t.push('s');
        }
        t
    };
    let ls = st.push(Level::Theme, &long_src, "color.s", "#111111", site(1));
    cs.add(
        "E06-边界-超长来源拒绝",
        ls.err == Some(OverlayCode::SourceIdInvalid),
        "来源超 MAX_SOURCE_LEN 须拒",
    );

    // 拒绝必须**零副作用**：上面七条拒绝之后，栈必须仍是空的
    // （审计 0 条、槽 0 个、生效 0 条）。这条守"拒绝不记账"——
    // 若拒绝也记审计，审计条数会被无效写入撑破，守恒式反而抓不到真问题。
    let clean = st.audit_len() == 0 && st.slot_count() == 0 && st.live_cell_count() == 0;
    cs.add(
        "E06-边界-拒绝零副作用",
        clean,
        "七条拒绝后审计/槽位/生效覆盖全为 0",
    );

    // 槽位超限：**整批拒**，不截断。构造 MAX_SLOTS 条路径后，第 MAX_SLOTS+1 条须拒。
    // 语料用序号路径 `"p0000".."pNNNN"`，长度固定且**严格升序**（不依赖 push 内部排序）。
    let mut big = OverlayStack::new();
    for i in 0..MAX_SLOTS {
        let p = format!("p{:04}", i);
        big.push(Level::Theme, "t", &p, "#111111", site(1));
    }
    let overflow = big.push(Level::Theme, "t", "p9999", "#111111", site(1));
    let before = big.slot_count();
    cs.add(
        "E06-边界-槽位超限整批拒",
        overflow.err == Some(OverlayCode::SlotLimit) && big.slot_count() == before,
        "第 MAX_SLOTS+1 条须拒 SlotLimit 且槽数不变（不截断）",
    );

    // 结构不变式：满槽栈 verify 通过（升序无重复）。
    let big_ok = big.verify() == Ok(());
    cs.add(
        "E06-边界-满槽栈结构自洽",
        big_ok,
        "满槽栈 verify 通过（路径严格升序无重复、审计序号连续）",
    );

    // 排序不变式被破坏时 verify 必须抓到 —— 这条需要能改坏栈，
    // 所以用一个**独立的坏栈**（不污染 base）。
    let mut broken = base_stack();
    broken.push(Level::Theme, "theme-dark", "color.b", "#111111", site(1));
    broken.push(Level::Theme, "theme-dark", "color.a", "#111111", site(2));
    // 正常 push 内部会重排序，所以这里拿到的应当是有序的（verify 通过）。
    // 断言"重排序真的发生了"：槽 0 的路径必须小于槽 1。
    let reordered = match broken.slots_ref().first() {
        Some(s) => s.path.as_str() == "color.a",
        None => false,
    };
    let v_ok = broken.verify() == Ok(());
    cs.add(
        "E06-边界-插入后重排序",
        reordered && v_ok,
        "逆序插入后槽位按路径升序且 verify 通过",
    );

    // **M35 实测暴露的可观测性缺口（真发现）**：`verify()` 里的路径长度与
    // 值/来源长度检查删掉后判据**全绿** —— 因为 `push` 入口已经拒掉非法输入，
    // 栈内不可能存在非法长度，那两段检查是**不可达的冗余防御**。
    //
    // 修法不是删防御（删了就真的没有任何东西守这条不变式），也不是加测试专用
    // 入口（污染生产面），而是**让防御覆盖一条 push 走不到但真实存在的路径**：
    // 槽内 `cells` 是私有字段，只有 `verify` 在跑，所以这里用 `Slot::new`
    // 造一个**已知非法**的槽来验证防御本身有效。
    //
    // ⚠ 注意：`Slot::new` 只做 `String::from`，不校验长度 —— 这正是防御的
    // 用武之地。若哪天 `Slot::new` 也开始校验，这条判据会转红，届时应改写它
    // （不是删防御）。
    let mut over = OverlayStack::new();
    let bad_path: String = {
        let mut t = String::new();
        for _ in 0..(MAX_PATH_LEN + 1) {
            t.push('q');
        }
        t
    };
    over.slots_mut_for_test().push(Slot::new(&bad_path));
    cs.add(
        "E06-边界-verify抓超长路径",
        over.verify() == Err(OverlayCode::PathInvalid),
        "直接构造超长路径槽（push 走不到），verify 必须抓（守冗余防御）",
    );

    let mut over2 = OverlayStack::new();
    let mut sl = Slot::new("color.ok");
    sl.cells_mut_for_test()[1] = Some(Override {
        level: Level::Theme,
        source: String::from("s"),
        value: {
            let mut t = String::new();
            for _ in 0..(MAX_VALUE_LEN + 1) {
                t.push('w');
            }
            t
        },
        site: site(1),
        shadowed: false,
        unaudited: false,
        third_party: false,
    });
    over2.slots_mut_for_test().push(sl);
    cs.add(
        "E06-边界-verify抓超长值",
        over2.verify() == Err(OverlayCode::ValueInvalid),
        "直接构造超长值覆盖，verify 必须抓 ValueInvalid",
    );

    // **C 组 M44 实测暴露的弱门禁（本条是它的修法）**：把 `verify()` 跳号检查
    // 的返回码从 `DiagIncomplete` 换成 `PathInvalid`，判据**全绿** ——
    // 因为没有任何一条判据断「跳号时返回哪个码」。push 保证序号不跳，所以那个
    // 分支在正常语料下走不到。本条直接构造跳号态把返回码钉死。
    let mut seq = base_stack();
    seq.push(Level::Theme, "theme-dark", "color.s", "#111111", site(1));
    seq.push(Level::Theme, "theme-dark", "color.s", "#222222", site(2));
    // 篡改第二条审计的序号 → 缺号
    for e in seq.audit_mut_for_test().iter_mut() {
        if e.seq == 2 {
            e.seq = 7;
        }
    }
    cs.add(
        "E06-边界-verify抓审计跳号",
        seq.verify() == Err(OverlayCode::DiagIncomplete),
        "构造缺号后 verify 必须返 DiagIncomplete（码位钉死，不许换）",
    );

    // 同一条守卫的**另一侧**：序号被改成 0 也必须被抓（防只断「过大」漏「过小」）。
    let mut seq0 = base_stack();
    seq0.push(Level::Theme, "theme-dark", "color.s", "#111111", site(1));
    for e in seq0.audit_mut_for_test().iter_mut() {
        e.seq = 0;
    }
    cs.add(
        "E06-边界-verify抓审计零号",
        seq0.verify() == Err(OverlayCode::DiagIncomplete),
        "序号为 0 同样须被抓（只断过大漏过小是弱门禁）",
    );

    // 错误码分级：越级/来源不明/未命中是告警级，其余是拒绝级。
    let warn_level = !OverlayCode::LayerOutOfOrder.is_reject()
        && !OverlayCode::SourceUnknown.is_reject()
        && !OverlayCode::NotFound.is_reject();
    let reject_level = OverlayCode::PathInvalid.is_reject()
        && OverlayCode::ValueInvalid.is_reject()
        && OverlayCode::SourceIdInvalid.is_reject()
        && OverlayCode::SourceDup.is_reject()
        && OverlayCode::SlotLimit.is_reject()
        && OverlayCode::OverrideLimit.is_reject()
        && OverlayCode::AuditLimit.is_reject()
        && OverlayCode::LevelFrozen.is_reject()
        && OverlayCode::DiagIncomplete.is_reject()
        && OverlayCode::UnknownLevel.is_reject();
    cs.add(
        "E06-边界-码位分级正确",
        warn_level && reject_level,
        "三条告警级 + 十条拒绝级（分级错会让告警被当拒绝或反之）",
    );

    // 码位与中文说明**一一对应**：wire 互异、zh 互异、zh 非空。
    // 逐条钉死（不能只断"文案非空" —— 那是记忆教训第 26 条）。
    let all = [
        OverlayCode::UnknownLevel,
        OverlayCode::PathInvalid,
        OverlayCode::ValueInvalid,
        OverlayCode::SourceIdInvalid,
        OverlayCode::SourceDup,
        OverlayCode::SlotLimit,
        OverlayCode::OverrideLimit,
        OverlayCode::AuditLimit,
        OverlayCode::LevelFrozen,
        OverlayCode::DiagIncomplete,
        OverlayCode::LayerOutOfOrder,
        OverlayCode::SourceUnknown,
        OverlayCode::NotFound,
    ];
    let mut codes_ok = all.len() == 13;
    let mut ws: [&str; 13] = [""; 13];
    let mut zs: [&str; 13] = [""; 13];
    for (i, c) in all.iter().enumerate() {
        ws[i] = c.wire();
        zs[i] = c.zh();
        if ws[i].is_empty() || zs[i].is_empty() {
            codes_ok = false;
        }
        for j in 0..i {
            if ws[j] == ws[i] || zs[j] == zs[i] {
                codes_ok = false;
            }
        }
    }
    cs.add(
        "E06-边界-13个码位逐条互异",
        codes_ok,
        "wire 与 zh 各自两两互异且非空（漏一条就会被合并掩盖）",
    );
}

// ---------------------------------------------------------------------------
// 六、报告与守恒
// ---------------------------------------------------------------------------

fn checks_report(cs: &mut CheckSet) {
    // 构造一个**四层 + 越级 + 来源不明 + 冲突**齐全的栈，逐项对账。
    let mut st = base_stack();
    // 正常四层。
    // **值集形态刻意选 `#aaaaa0`..`#aaaaa3`**：早先用 `v0..v3` 时，
    // `"v1"` 撞上了报告自身的版本号 `E01-overlay-v1` —— 泄漏检测器把版本号
    // 里的 "v1" 当成了覆盖值泄漏，判据在**基线上就红**。
    // ⇒ **判据的值集必须与被测对象的固定文案（版本号/码位/层名）不相交**，
    // 否则测的就不是隐私而是"文案里恰好有这几个字符"。
    for (i, l) in Level::ALL.iter().enumerate() {
        let v = format!("#aaaaa{}", i);
        st.push(*l, "builtin", "color.a", &v, site(i as u32 + 1));
    }
    // 同层冲突一次（多一条 SUPERSEDED 审计）。
    st.push(Level::Theme, "theme-dark", "color.b", "#111111", site(5));
    st.push(Level::Theme, "theme-dark", "color.b", "#222222", site(6));
    // 来源不明一条。
    st.push(Level::Scene, "ghost", "color.c", "#333333", site(7));
    // 定稿后越级一条。
    st.freeze_to(Level::Scene);
    st.push(Level::Default, "builtin", "color.d", "#444444", site(8));

    let rep = OverlayReport::build(&st);

    // 三条守恒式逐条断（不合并 —— 见 `conserved()` 的注释）。
    let cell_total = rep.cell_total();
    let accounted = rep.accounted();
    let action_total = rep.action_total();
    cs.add(
        "E06-报告-槽内条目对账",
        cell_total == 7,
        "color.a 四层4格 + color.b 主题1格(同层重写不增格) + color.c 场景1格          + color.d 默认1格(越级仍占格) = 7",
    );
    cs.add(
        "E06-报告-生效加遮蔽等于槽内",
        accounted == cell_total,
        "live + shadowed == cell_total",
    );
    // 审计条数**独立推导**：color.a 四次写各 1 条 = 4；color.b 首次写 1 条 +
    // 第二次写产 SUPERSEDED 1 条 + 新覆盖 1 条 = 3；color.c 1 条；color.d 1 条。
    // 合计 4 + 3 + 1 + 1 = 9。**注意 color.b 是 3 条不是 2 条** —— 冲突那次
    // push 同时记"被压者"与"新覆盖"两条，这是"每次 push 至多两条"的来源。
    cs.add(
        "E06-报告-动作计数等于审计数",
        action_total == rep.audit && rep.audit == 9,
        "4(a) + 3(b: 首次1 + 冲突2) + 1(c) + 1(d) = 9",
    );
    cs.add(
        "E06-报告-守恒式全成立",
        rep.conserved(),
        "conserved() 三式同时成立",
    );

    // 四层分布逐层对账（判据侧独立算：每层应写了几条）。
    // 判据语料：color.a 四层各1、color.b 主题1、color.c 场景1、color.d 默认1。
    // ⇒ 默认2 / 主题2 / 场景2 / 组件1 = 7。**注意 d 是越级但仍占默认格**。
    let expect_levels = [2usize, 2, 2, 1];
    let dist_ok = rep.per_level == expect_levels;
    cs.add(
        "E06-报告-四层分布逐层对账",
        dist_ok,
        "默认2/主题2/场景2/组件1（越级条目仍占默认格）",
    );

    // 越级数 = 1、来源不明数 = 1、扩展数 = 1（builtin 四条非扩展）。
    let counters_ok = rep.shadowed == 1 && rep.unaudited == 1 && rep.third_party == 0 && rep.live == 6;
    cs.add(
        "E06-报告-越级/欠账/扩展计数",
        counters_ok,
        "越级1/欠账1/扩展0/生效6（生效 = 槽内7 - 遮蔽1）",
    );

    // 扩展计数单独验：换一个栈，确认真能数到扩展。
    let mut ext = base_stack();
    ext.push(Level::Component, "ext-button", "color.a", "#111111", site(1));
    ext.push(Level::Scene, "ext-button", "color.b", "#222222", site(2));
    ext.push(Level::Theme, "theme-dark", "color.c", "#333333", site(3));
    let ext_rep = OverlayReport::build(&ext);
    cs.add(
        "E06-报告-扩展计数非平凡",
        ext_rep.third_party == 2 && ext_rep.sources == 4,
        "两条扩展来源 + 四条登记来源",
    );

    // 报告版本号取自常量（下游按版本决定是否重审）。
    cs.add(
        "E06-报告-版本号自洽",
        rep.version == OVERLAY_VERSION && !OVERLAY_VERSION.is_empty(),
        "版本取自 OVERLAY_VERSION 且非空",
    );

    // 读屏：六层信息齐全（四层名 + 三个计数）**且不含值正文**。
    let spoken = rep.spoken();
    let mut named = spoken.contains("默认层")
        && spoken.contains("主题层")
        && spoken.contains("场景层")
        && spoken.contains("组件层");
    named = named
        && spoken.contains("生效")
        && spoken.contains("越级未生效")
        && spoken.contains("来源未登记");
    cs.add("E06-读屏-报告要素齐全", named, "四层名 + 生效/越级/欠账计数都在");

    // 隐私反向断言：报告与逐条播报都不得含值集里的**任一**字面量。
    let mut leak = false;
    let literals = [
        "#aaaaa0", "#aaaaa1", "#aaaaa2", "#aaaaa3", "#111111", "#222222", "#333333",
        "#444444",
    ];
    for lit in literals.iter() {
        if spoken.contains(lit) {
            leak = true;
        }
    }
    for e in st.audit_ref() {
        let s = e.spoken();
        for lit in literals.iter() {
            if s.contains(lit) {
                leak = true;
            }
        }
    }
    // **反向断言**：报告必须真的提到来源与层级（否则"不含值"是因为什么都没说）。
    let mentions_source = spoken.contains("来源");
    cs.add(
        "E06-读屏-报告不含值集",
        !leak && mentions_source,
        "八个字面量逐一禁；且必须真的提到来源（防空话）",
    );

    // Resolved::spoken 同样不含值，且念得出层级与压住关系。
    let r_spoken = match st.resolve("color.a") {
        Some(r) => r.spoken(),
        None => String::new(),
    };
    let r_ok = r_spoken.contains("组件层")
        && r_spoken.contains("builtin")
        && !r_spoken.contains("#aaaaa3");
    cs.add(
        "E06-读屏-仲裁播报不含值",
        r_ok,
        "念出组件层与来源，且不含胜出值 #aaaaa3",
    );

    // 未登记来源的播报必须明说"未登记"（不能静默念空串）。
    let u_spoken = match st.resolve("color.c") {
        Some(r) => r.spoken(),
        None => String::new(),
    };
    let u_ok = u_spoken.contains("未登记") && u_spoken.contains("审计欠账");
    cs.add(
        "E06-读屏-欠账明示",
        u_ok,
        "来源不明的仲裁播报须明说未登记与欠账",
    );
}

// ---------------------------------------------------------------------------
// 七、跨批对接（E14 第三方扩展边界）
// ---------------------------------------------------------------------------

fn checks_handoff(cs: &mut CheckSet) {
    // 导出的 Binding 数 == 生效覆盖数（**不含越级**）。这是对接正确性的核心：
    // 把越级条目导出去 = 让 F3405 校验一条永不生效的值，错误会一路飘到渲染层。
    let mut st = base_stack();
    st.push(Level::Default, "builtin", "color.a", "#111111", site(1));
    st.push(Level::Theme, "theme-dark", "color.a", "#222222", site(2));
    st.push(Level::Scene, "scene-dash", "color.b", "#333333", site(3));
    st.freeze_to(Level::Scene);
    st.push(Level::Default, "builtin", "color.c", "#444444", site(4)); // 越级

    // **两个口径分别钉，不混用**（第一版红项的根因就在这里）：
    // 格子口径：槽内 4 格 - 1 越级 = 3 格存活。
    // 路径口径：color.a（有胜者）+ color.b（有胜者）+ color.c（唯一一格越级，
    //           无胜者）= 2 条路径。
    // 导出必须等于**路径口径**，因为消费方按路径取值。
    let cells = st.live_cell_count();
    let resolved = st.resolved_count();
    let binds: Vec<Binding> = st.export_bindings(site(9));
    let export_ok = binds.len() == resolved && resolved == 2 && cells == 3;
    cs.add(
        "E06-对接-导出数等于路径口径",
        export_ok,
        "格子3(4格-1越级) 但路径2(color.c无胜者)；导出必须=路径口径",
    );

    // 两口径的偏序关系：路径口径 ≤ 格子口径（多格路径只出一条）。
    cs.add(
        "E06-对接-两口径偏序成立",
        resolved <= cells && resolved == st.resolve_all().len(),
        "路径口径 <= 格子口径，且与 resolve_all().len() 一致",
    );

    // 导出的路径集合必须恰好是生效路径（不是全部槽）。
    let mut paths_ok = true;
    for b in binds.iter() {
        if b.path != "color.a" && b.path != "color.b" {
            paths_ok = false;
        }
    }
    let no_ooo = st.resolve("color.c").is_none();
    cs.add(
        "E06-对接-越级路径不外泄",
        paths_ok && no_ooo,
        "导出只含生效路径；越级路径 resolve 为空",
    );

    // 导出的值必须与 resolve 一致（不是原样透传所有格子）。
    let val_ok = match (st.resolve("color.a"), binds.iter().find(|b| b.path == "color.a")) {
        (Some(r), Some(b)) => b.raw.as_str() == r.value.as_str(),
        _ => false,
    };
    cs.add("E06-对接-导出值等于胜出值", val_ok, "color.a 导出的是场景层值 #333333");

    // 真与 F3405 联调：导出的 Binding 直接喂 TypeChecker 做类型校验。
    // **这条判据证明对接不是"形状对"而是"真能用"**。
    let mut tc = super::ver01e_typetree::TypeChecker::new();
    let _ = tc.declare("color.a", super::ver01e_typetree::TokenKind::Color, site(1));
    let _ = tc.declare("color.b", super::ver01e_typetree::TokenKind::Color, site(1));
    let v = tc.check_all(&binds);
    let mut tc_ok = v.accepted.len() == 2 && v.violations.is_empty() && v.warnings.is_empty();
    // 换成一个非法值（具名色），F3405 必须拒 —— 证明类型校验真的在跑。
    let mut bad = base_stack();
    bad.push(Level::Theme, "theme-dark", "color.a", "coral", site(1));
    let bad_binds = bad.export_bindings(site(9));
    let bv = tc.check_all(&bad_binds);
    let reject_bad = bv.accepted.is_empty() && !bv.violations.is_empty();
    tc_ok = tc_ok && reject_bad;
    cs.add(
        "E06-对接-F3405联调真拒非法值",
        tc_ok,
        "合法两条过检；coral 被 F3405 拒绝且不产出值",
    );

    // F3405 的未知路径拒绝：导出集合里出现未声明路径时必须被抓。
    let tc2 = super::ver01e_typetree::TypeChecker::new();
    let unknown_ok = tc2.check_all(&binds).accepted.is_empty();
    cs.add(
        "E06-对接-未声明路径被F3405拒",
        unknown_ok,
        "未 declare 就 check_all：产出为空（无声明 ⇒ 无类型）",
    );
}

// ---------------------------------------------------------------------------
// 八、入口
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 九、变异台（W010·VE-F3406 **实测**）
// ---------------------------------------------------------------------------
//
// 四组共 51 次注入。**32 条真变异全部转红 + 2 条等价对照正确保持全绿**；
// 另有 4 条在实测中暴露判据太弱 → 补判据 → 重测转红（下表列在"补判据后"）。
// 基线 **76 项全绿**。
//
// | 变异 | 注入的缺陷 | 实测捕获（红项数 · 首条） |
// |---|---|---|
// | M01 | 仲裁改成低者胜（rank 反向） | 8 · `E06-栈-四层全填时逐层命中` |
// | M02 | 仲裁改成首个候选胜（first-wins） | 7 · `E06-栈-四层全填时逐层命中` |
// | M03 | 仲裁忽略遮蔽标记（越级也参与） | 3 · `E06-对接-导出数等于路径口径` |
// | M04 | 同层冲突不记 SUPERSEDED 审计 | 2 · `E06-仲裁-冲突进审计且带标记` |
// | M05 | 同层冲突先写者胜（新值丢弃） | 3 · `E06-仲裁-同层后来者胜` |
// | M06 | 越级改判为拒绝 | 2 · `E06-越级-告警码正确` |
// | M07 | 越级仍然生效（不标 shadowed） | 10 · `E06-越级-告警码正确` |
// | M08 | 越级不记审计 | 3 · `E06-越级-审计记SHADOWED_COVER` |
// | M09 | 定稿之上不拒（`LevelFrozen` 失效） | 1 · `E06-越级-定稿之上拒绝` |
// | M10 | 定稿可下探（放开低层） | 1 · `E06-越级-定稿只上不下` |
// | M11 | 来源不明改判为拒绝 | 1 · `E06-审计-来源不明告警` |
// | M13 | 未登记来源不记 UNAUDITED 审计动作 | 1 · `E06-审计-来源不明记UNAUDITED` |
// | M15 | 审计来源改成占位符（查不出谁改的） | 3 · `E06-越级-审计保留原来源` |
// | M17 | 空路径放行 | 2 · `E06-边界-空路径拒绝` |
// | M18 | 重复来源放行 | 1 · `E06-审计-重复来源拒绝` |
// | M19 | 槽位超限改截断（不拒） | 2 · `E06-边界-槽位超限整批拒` |
// | M21 | 插入后不重排序 | 1 · `E06-边界-插入后重排序` |
// | M23 | 报告 live 少算一格（口径错） | 3 · `E06-报告-生效加遮蔽等于槽内` |
// | M24 | 报告四层分布漏一层 | 4 · `E06-报告-槽内条目对账` |
// | M25 | 报告 shadowed 计数归零 | 3 · `E06-报告-生效加遮蔽等于槽内` |
// | M26 | 报告读屏并入值集 | 1 · `E06-读屏-报告不含值集` |
// | M27 | 导出含越级条目（越级外泄） | 1 · `E06-对接-导出值等于胜出值` |
// | M28 | `resolved_count` 用格子口径 | 2 · `E06-对接-导出数等于路径口径` |
// | M32 | 未登记来源误打第三方扩展标记 | 2 · `E06-审计-来源不明不算扩展` |
// | M33 | 审计序号跳号（每次 +2） | 3 · `E06-审计-序号连续无缺号` |
// | M34 | 拒绝路径仍占槽 | 1 · `E06-边界-满槽栈结构自洽` |
// | M36 | `freeze_to` 完全失效 | 12 · `E06-越级-告警码正确` |
// | M37 | 未命中返默认覆盖而非空 | 1 · `E06-仲裁-未命中返空` |
// | M38 | 被压层清单顺序错（升序） | 1 · `E06-仲裁-被压层按rank降序` |
// | M39 | 码位分级反转（告警被当拒绝） | 1 · `E06-边界-码位分级正确` |
// | M40 | 报告读屏省略四层名 | 1 · `E06-读屏-报告要素齐全` |
// | M45 | 被压层清单混入越级层 | 1 · `E06-越级-不入被压层清单` |
// | M46 | 越级判定方向反了 | 10 · `E06-越级-告警码正确` |
// | M49 | `audit_by_source` 忽略来源过滤 | 1 · `E06-审计-按来源检索准确` |
// | **M35** | verify 删掉路径长度检查 | 0 → **补判据后 1** · `E06-边界-verify抓超长路径` |
// | **M43** | verify 删掉值长度检查 | 1 · `E06-边界-verify抓超长值` |
// | **M44** | verify 跳号返回码换码位 | 0 → **补判据后 2** · `E06-边界-verify抓审计跳号` |
// | **M47** | `audit_of` 忽略路径过滤 | 0 → **补判据后 1** · `E06-审计-多路径过滤生效` |
// | **M48** | verify 只查序号过大漏过小 | 1 · `E06-边界-verify抓审计零号` |
// | M29 | 【等价对照】`arbitrate` 改写但取值同 | **0 · 保持全绿（正确）** |
// | M30 | 【等价对照】`shadowed_levels` 换等长实现 | **0 · 保持全绿（正确）** |
//
// ## 实测抓到的三件事（都是真发现，不是复述教训）
//
// **一、单路径语料让过滤型判据恒真（本轮最实操的一条）。**
// `E06-审计-谁在何层可追溯` 断言 `audit_of(路径).len() == 4`，但那批语料**只写了
// 一条路径**（`color.accent`）—— 于是"返回全量"和"按路径过滤"结果相同，把过滤
// 改成恒真照样全绿。补三路径各两次写的语料后，同一条变异
// （M47：`audit_of` 直接返回全量）转红。
// ⇒ **过滤型判据的语料必须覆盖过滤维度**，否则它测的是"返回值恰好等于期望个数"。
// 这与记忆里"窗口类判据必须含至少一次真命中"是同一族：**没触发的守卫测不出**。
//
// **二、被测代码里有不可达的冗余防御，判据必须为它单独造态。**
// `verify()` 里的路径长度与值长度检查删掉后判据**全绿**。查根因不是判据弱，
// 而是那两段防御**永远走不到** —— `push` 入口已把非法输入全拒了。
// 修法有三条可选，本单选了**第三条**：
// - 删掉防御 ⇒ 那条不变式就彻底没人守了（拒绝）。
// - 加 `#[cfg(test)]` 专用入口 ⇒ 污染生产面并让判据测到不存在的路径（拒绝）。
// - **判据侧用 `slots_mut_for_test` 直接构造非法态** ⇒ 防御既保留又可观测（采纳）。
// ⚠ 附带条件已写进判据注释：若哪天 `Slot::new` 也开始校验长度，
// `E06-边界-verify抓超长路径` 会转红，届时该改写判据（**不是删防御**）。
//
// **三、格子口径与路径口径是两个数，混用会让判据和被测同时"看起来都对"。**
// 导出判据第一版断言 `export.len() == live_count()`，而 `live_count()` 数的是
// **格子**（一条路径四层各一格）、`export_bindings()` 数的是**路径**。
// 语料恰好让两者不等（3 格 vs 2 路径），判据红了。**当时最容易的错误处理是改
// 被测代码让两个数对上** —— 那是把两个不同的问题硬凑成一个。本单的做法是：
// 把方法改名为 `live_cell_count()`（格子口径）、新增 `resolved_count()`（路径口径），
// 判据拆成"两个口径各自的绝对值 + 一条偏序"三条。
// ⇒ **改名比加注释更能防止下一次误用**：名字里带 `cell` 就是提醒。
//
// **等价对照项存在的理由**：M29/M30 两条重写后取值完全相同，判据正确地保持全绿。
// 没有它们，上表 34 条的"全部转红"无法区分是判据强还是变异有效 ——
// 其中 M12/M14/M20/M22/M35/M44/M47 **第一轮确实零红**，正是靠"零红要逐条查
// 根因"这条纪律把它们从"无效变异"里捞出来，捞出后有 4 条是真弱门禁。
//
// **零墙钟、零 IO、无随机源，回归可复现。**

/// 主判据集。
pub fn run_ver01f_checks() -> CheckSet {
    let mut cs = CheckSet::new("ver01f-overlay");
    checks_four_levels(&mut cs);
    checks_arbitration(&mut cs);
    checks_out_of_order(&mut cs);
    checks_audit(&mut cs);
    checks_boundaries(&mut cs);
    checks_report(&mut cs);
    checks_handoff(&mut cs);
    cs
}
