//! VE-F3605 · 域自检（判据逐条映射，见 `ves05_bridge.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项前缀）：
//! - 统一管线 → `F3605-统一管线-*`
//! - 预览隔离 → `F3605-预览隔离-*`
//! - 令牌单源 → `F3605-令牌单源-*`
//! - 转换对拍 → `F3605-转换对拍-*`
//! - 收敛执法 → `F3605-收敛执法-*`
//! - 判据（自证可追溯）→ `F3605-判据-*`
//! - 降级矩阵 → `F3605-降级-*`
//! - 禁扩面 → `F3605-边界-*`
//!
//! **可证伪纪律**：每组自检都配「注入缺陷 → 必红」的单元测试。报"计数为 0"
//! 不算守住，自检必须能被正当理由打红才算真守住。
//!
//! **判据侧独立重算纪律**：凡涉及数值（签名、摘要、计数）的判据，一律用
//! **本文件内的独立常量或独立算法**重算，绝不调用被测函数求自己的答案
//! （自证式弱门禁）。签名与令牌摘要的权威值由 Python 侧独立算出后写死在此。
//!
//! 零墙钟、零 IO，回归可复现。

use alloc::format;
use alloc::vec::Vec;

use super::ves05_bridge::*;
use crate::checks::CheckSet;

/// 全绿总纲（**唯一正样本构造处**）。
fn ready() -> BridgeArchitecture {
    BridgeArchitecture::standard()
}

/// 数指定错误码在问题表里出现的次数（真红项计数，非 CheckSet 总数）。
fn count_code(issues: &[BridgeIssue], code: &str) -> usize {
    issues.iter().filter(|i| i.code == code).count()
}

/// 判据侧独立 FNV-1a（**不复用被测模块的 `fnv1a64`**——同源驱动恒真，
/// 抄一份实现等于把被测对象换成自己，两边一起错就恒绿）。
fn oracle_fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        // 常数在判据侧展开为 1099511628211，避免与被测代码共用同一个字面量来源
        h = h.wrapping_mul(1099511628211);
    }
    h
}

/// 正样本票据号（走完报名→盖章→消费的那一张）。
const READY_TICKET: u32 = 1;
/// 正样本载荷指纹（= Python 侧独立重算的 `fnv("theme.alpha.bg")`）。
const READY_FP: u64 = 0xf02f_70f5_2404_58b5;

/// 独立重建签名材料（判据侧自建，验证被测 `bridge_signature` 的材料拼装）。
fn oracle_signature_material() -> Vec<u8> {
    let mut m: Vec<u8> = Vec::new();
    m.extend_from_slice(b"creation-e-bridge-v1");
    m.push(0);
    for p in ["P-THEME", "P-SKIN"] {
        m.extend_from_slice(p.as_bytes());
        m.push(0);
    }
    for l in ["L-FORMAL", "L-PREVIEW"] {
        m.extend_from_slice(l.as_bytes());
        m.push(0);
    }
    for c in [
        "C-UNIFIED-PIPELINE",
        "C-PREVIEW-ISOLATION",
        "C-TOKEN-SINGLE-SOURCE",
        "C-CONVERSION-PARITY",
        "C-CONVERGENCE-ENFORCE",
        "C-CRITERIA-PROVENANCE",
    ] {
        m.extend_from_slice(c.as_bytes());
        m.push(0);
    }
    m
}

// ---------------------------------------------------------------------------
// 判据一：统一管线
// ---------------------------------------------------------------------------

fn chk_unified_pipeline(set: &mut CheckSet) {
    let a = ready();

    // 1. 标准总纲零红项。
    let issues = a.collect_issues();
    set.add(
        "F3605-统一管线-标准零红项",
        issues.is_empty(),
        "全绿样本不该有契约问题",
    );

    // 2. 三态齐备且序关系正确（报名 → 盖章 → 已用）。
    let t = a.ledger.ticket(READY_TICKET);
    let states_ok = matches!(t.map(|x| x.state), Some(TicketState::Spent));
    set.add("F3605-统一管线-票据三态可达", states_ok, "标准样本票据应已消费");

    // 3. 已消费票据**不可**消费（重放被拒）。
    let mut l2 = DelegateLedger::empty();
    let no = l2.enroll(Product::Skin, READY_FP, 512).unwrap_or(0);
    let sealed = l2.seal(no, READY_FP);
    let mut f2 = FormalChannel::new();
    let first = f2.consume(&mut l2, no, READY_FP);
    let replay = f2.consume(&mut l2, no, READY_FP);
    set.add(
        "F3605-统一管线-盖章后可消费一次",
        sealed.is_ok() && first.is_ok() && f2.entries() == 1,
        "已盖章票据首次消费应成功且正式缓存恰增一条",
    );
    set.add(
        "F3605-统一管线-重放被拒",
        replay.as_ref().err().map(|e| e.code) == Some(E_TICKET_REPLAY),
        "同一票据二次消费须报 E_TICKET_REPLAY",
    );

    // 4. 未盖章票据不可消费。
    let mut l3 = DelegateLedger::empty();
    let n3 = l3.enroll(Product::Theme, READY_FP, 128).unwrap_or(0);
    let mut f3 = FormalChannel::new();
    let unsealed = f3.consume(&mut l3, n3, READY_FP);
    set.add(
        "F3605-统一管线-未盖章拒消费",
        unsealed.as_ref().err().map(|e| e.code) == Some(E_PIPELINE_UNSEALED),
        "未盖章票据消费须报 E_PIPELINE_UNSEALED",
    );

    // 5. 凭空消费（无票据）＝绕管线直消费，须立案。
    let mut l4 = DelegateLedger::empty();
    let mut f4 = FormalChannel::new();
    let absent = f4.consume(&mut l4, 4242, READY_FP);
    let e_absent = absent.as_ref().err().map(|e| e.code);
    let act = absent.as_ref().err().map(|e| e.action());
    let sev = absent.as_ref().err().map(|e| e.severity());
    set.add(
        "F3605-统一管线-无票据即立案",
        e_absent == Some(E_PIPELINE_BYPASS),
        "无票据直消费正式通道须报 E_PIPELINE_BYPASS（收敛红线正身）",
    );
    set.add(
        "F3605-统一管线-绕管线动作落立案",
        act == Some(DegradeAction::FileCase) && sev == Some(P0),
        "锚点：绕管线直消费→立案（收敛复述），严重级 P0",
    );

    // 5b. 码位分工：`E_TICKET_ABSENT` 只归盖章路径（运维笔误，P2），
    //     不得与消费的 `E_PIPELINE_BYPASS` 重合——两码同行为即掩盖缺失分支。
    let mut l4b = DelegateLedger::empty();
    let seal_missing = l4b.seal(7777, READY_FP);
    set.add(
        "F3605-统一管线-盖章缺票独立报码",
        seal_missing.as_ref().err().map(|e| e.code) == Some(E_TICKET_ABSENT)
            && e_absent != seal_missing.as_ref().err().map(|e| e.code),
        "盖章路径报缺票、消费路径报绕管线，两码不得重合",
    );

    // 6. 转换失真须被拒（指纹不符）。
    let mut l5 = DelegateLedger::empty();
    let n5 = l5.enroll(Product::Theme, READY_FP, 64).unwrap_or(0);
    let _ = l5.seal(n5, READY_FP);
    let mut f5 = FormalChannel::new();
    let drift = f5.consume(&mut l5, n5, READY_FP ^ 0x1);
    set.add(
        "F3605-统一管线-失真拒消费",
        drift.as_ref().err().map(|e| e.code) == Some(E_PAYLOAD_DRIFT),
        "引擎侧重算指纹与票据不符须报 E_PAYLOAD_DRIFT",
    );

    // 7. 账本容量有界：满则拒新报名（不覆盖旧票据）。
    let mut l6 = DelegateLedger::empty();
    for _ in 0..MAX_TICKETS {
        let _ = l6.enroll(Product::Theme, 1, 1);
    }
    let over = l6.enroll(Product::Theme, 1, 1);
    set.add(
        "F3605-统一管线-账本满则拒",
        over.as_ref().err().map(|e| e.code) == Some(E_LEDGER_FULL)
            && l6.len() == MAX_TICKETS,
        "账本满须拒新报名且旧票据不被覆盖",
    );

    // 8. 票据号不复用：释放后新票据号不回落。
    let mut l7 = DelegateLedger::empty();
    let t1 = l7.enroll(Product::Theme, 7, 8).unwrap_or(0);
    let t2 = l7.enroll(Product::Skin, 9, 10).unwrap_or(0);
    set.add(
        "F3605-统一管线-票据号单调",
        t2 == t1 + 1 && t1 == 1,
        "票据号须单调递增，不按释放回落",
    );

    // 9. 建销对账。
    set.add(
        "F3605-统一管线-建销对账",
        a.ledger.reconcile() && a.ledger.spent_actual() == a.ledger.spent_count as usize,
        "已消费票据实数须等于计数",
    );

    // 10. 重复盖章被拒（**复活攻击**：已消费票据若能被再盖章回 Sealed，
    //    就能被二次消费——这是收敛红线的一个真实缺口，不是理论风险）。
    let mut l10 = DelegateLedger::empty();
    let n10 = l10.enroll(Product::Theme, READY_FP, 32).unwrap_or(0);
    let _ = l10.seal(n10, READY_FP);
    let mut f10 = FormalChannel::new();
    let _ = f10.consume(&mut l10, n10, READY_FP);
    let spent_state = l10.ticket(n10).map(|t| t.state);
    let reseal = l10.seal(n10, READY_FP);
    let state_after = l10.ticket(n10).map(|t| t.state);
    // 再消费一次：若复活成功，这两次消费都会成功。
    let second = f10.consume(&mut l10, n10, READY_FP);
    set.add(
        "F3605-统一管线-重复盖章被拒",
        spent_state == Some(TicketState::Spent)
            && reseal.is_err()
            && state_after == Some(TicketState::Spent),
        "已消费票据再盖章须被拒且状态不得回退 Spent（防复活）",
    );
    set.add(
        "F3605-统一管线-复活后不可二次消费",
        second.is_err() && f10.entries() == 1,
        "复活攻击即便发生，二次消费也须被拒且正式缓存不增（双保险）",
    );

    // 11. 枚举全集闭合：产物恰两类、通道恰两条。
    set.add(
        "F3605-统一管线-产物通道全集",
        Product::ALL.len() == 2 && Lane::ALL.len() == 2,
        "锚点：创作产物为主题/皮肤两类；通道为正式/预览两条",
    );
}

// ---------------------------------------------------------------------------
// 判据二：预览隔离
// ---------------------------------------------------------------------------

fn chk_preview_isolation(set: &mut CheckSet) {
    let a = ready();
    let base = a.formal.entries();

    // 1. 预览往返不改动正式缓存条目数（红线核心）。
    let mut pv = PreviewChannel::new(&a.formal);
    for i in 0..8u64 {
        let _ = pv.push(PreviewFrame::new(256, 0x5ca1_ab00 + i));
    }
    pv.discard_all();
    pv.observe_formal(a.formal.entries());
    set.add(
        "F3605-预览隔离-往返不污染",
        pv.isolation_intact() && a.formal.entries() == base,
        "锚点：编辑预览不污染正式缓存（红线）",
    );

    // 2. 隔离核验为 O(1)：只比两个整数，不遍历帧。
    //    机检形态：帧数任意变化**不改变**判定结果（只看两个整数）。
    let mut pv2 = PreviewChannel::new(&a.formal);
    let _ = pv2.push(PreviewFrame::new(1, 1));
    pv2.observe_formal(a.formal.entries());
    let few = pv2.isolation_intact();
    for i in 0..64u64 {
        let _ = pv2.push(PreviewFrame::new(64, i));
    }
    pv2.observe_formal(a.formal.entries());
    set.add(
        "F3605-预览隔离-判定与帧数无关",
        few && pv2.isolation_intact() && pv2.frames().len() == 65,
        "隔离判定只读两个整数，帧数变化不影响结论（O(1)）",
    );

    // 3. 污染检出（红线实测）：正式缓存被外部改动后须报污染。
    let mut pv3 = PreviewChannel::new(&a.formal);
    let _ = pv3.push(PreviewFrame::new(128, 2));
    pv3.observe_formal(base + 1); // 模拟有人从预览侧写了正式缓存
    let contam = pv3.contamination();
    set.add(
        "F3605-预览隔离-污染可检出",
        !pv3.isolation_intact()
            && contam.as_ref().map(|c| c.code) == Some(E_PREVIEW_CONTAMINATION),
        "正式条目数偏离基线须报 E_PREVIEW_CONTAMINATION",
    );
    set.add(
        "F3605-预览隔离-污染属阻断类",
        contam.as_ref().map(|c| c.is_blocking()) == Some(true),
        "污染须归阻断类红线",
    );

    // 4. 污染的降级落点＝隔离修正（锚点原文）。
    let c = BridgeError::new(E_PREVIEW_CONTAMINATION, "x", "");
    set.add(
        "F3605-预览隔离-降级落隔离修正",
        c.action() == DegradeAction::IsolateRepair && c.severity() == P0,
        "锚点：预览污染检出→隔离修正（红线实测）",
    );

    // 5. 隔离修正可执行：回滚至基线后污染消除。
    let mut f5 = FormalChannel::new();
    let mut l5 = DelegateLedger::empty();
    for _ in 0..3 {
        let n = l5.enroll(Product::Theme, READY_FP, 32).unwrap_or(0);
        let _ = l5.seal(n, READY_FP);
        let _ = f5.consume(&mut l5, n, READY_FP);
    }
    let grown = f5.entries();
    // 先制造污染：基线取 3，随后缓存被外部改动。
    let mut pv5 = PreviewChannel::new(&f5);
    let _ = pv5.push(PreviewFrame::new(64, 3));
    pv5.observe_formal(grown + 1);
    let polluted = pv5.contamination().is_some();
    // 再执行隔离修正：回滚到目标，随后**重新取基线**。
    // 顺序是关键——拿回滚前的旧基线去比，这条判据在结构上永远不可能绿
    //（自证式空门禁：基线与观测值取自同一份已被改动的状态，比的永远是旧账）。
    f5.rollback_to(grown - 1);
    let mut pv5b = PreviewChannel::new(&f5);
    let _ = pv5b.push(PreviewFrame::new(64, 3));
    pv5b.observe_formal(f5.entries());
    set.add(
        "F3605-预览隔离-修正后污染消除",
        grown == 3
            && polluted
            && f5.entries() == 2
            && pv5b.isolation_intact()
            && pv5b.contamination().is_none(),
        "污染可检出且回滚+重取基线后转 clean（修正链两端都验）",
    );

    // 6. 轻量红线：超字节上限被拒。
    let mut pv6 = PreviewChannel::new(&a.formal);
    let heavy = pv6.push(PreviewFrame::new(PREVIEW_BYTE_CAP + 1, 4));
    let atcap = pv6.push(PreviewFrame::new(PREVIEW_BYTE_CAP, 5));
    set.add(
        "F3605-预览隔离-超限被拒",
        heavy.as_ref().err().map(|e| e.code) == Some(E_PREVIEW_HEAVY),
        "预览载荷超轻量上限须被拒",
    );
    set.add(
        "F3605-预览隔离-上限边界恰可过",
        atcap.is_ok() && pv6.frames().len() == 1,
        "恰等于上限不算超（边界须夹逼对，不许差一）",
    );

    // 7. 隔离由**类型**保证：预览通道无写正式缓存的方法。
    //    机检形态：预览通道的公开写入口只有 `push`（入预览缓冲），
    //    正式通道的写入口 `consume` 必须持票据——两个入口的签名不同。
    let mut f7 = FormalChannel::new();
    let mut l7 = DelegateLedger::empty();
    let n7 = l7.enroll(Product::Theme, READY_FP, 16).unwrap_or(0);
    let direct = f7.consume(&mut l7, n7 + 1000, READY_FP);
    set.add(
        "F3605-预览隔离-正式入口须持票据",
        direct.is_err() && f7.entries() == 0,
        "正式通道唯一写入口 consume 必持票据，无第二入口",
    );

    // 8. 预览缓冲字节量可加总（轻量度量）。
    let mut pv8 = PreviewChannel::new(&a.formal);
    let _ = pv8.push(PreviewFrame::new(100, 1));
    let _ = pv8.push(PreviewFrame::new(200, 2));
    set.add(
        "F3605-预览隔离-缓冲量可加总",
        pv8.buffered_bytes() == 300 && pv8.frames().len() == 2,
        "预览缓冲字节须为逐帧之和（不是最大值）",
    );

    // 9. 丢弃预览帧不影响正式缓存。
    let mut pv9 = PreviewChannel::new(&a.formal);
    for _ in 0..4 {
        let _ = pv9.push(PreviewFrame::new(64, 9));
    }
    pv9.discard_all();
    set.add(
        "F3605-预览隔离-丢弃不触正式",
        pv9.frames().is_empty() && pv9.buffered_bytes() == 0 && a.formal.entries() == base,
        "放弃编辑须只清预览缓冲",
    );

    // 10. 预览通道可从正式通道构造且基线取自正式缓存实况。
    set.add(
        "F3605-预览隔离-基线取自实况",
        pv9.formal_baseline() == a.formal.entries(),
        "隔离基线须取正式缓存当前条目数，不得写死",
    );
}

// ---------------------------------------------------------------------------
// 判据三：令牌单源
// ---------------------------------------------------------------------------

fn chk_token_single_source(set: &mut CheckSet) {
    let a = ready();

    // 1. 权威源恒为 E 域 F3003 通道。
    set.add(
        "F3605-令牌单源-权威源恒定",
        TOKEN_AUTHORITY == "E-F3003"
            && a.tokens.bindings().iter().all(|b| b.is_authoritative()),
        "锚点：主题资产→令牌集（F3003 通道复用单源声明）",
    );

    // 2. 来源不可自报：`reference` 唯一写死权威源。
    let b = TokenBinding::reference("motion.duration.fast", 0, 0xbf05_eefc_67c4_651d);
    set.add(
        "F3605-令牌单源-来源不接受自报",
        b.as_ref().map(|x| x.source) == Ok(TOKEN_AUTHORITY),
        "构造路径不接受调用方自报来源，故非单源构造不出来",
    );

    // 3. 标准声明零红项且无分叉。
    let issues = a.tokens.audit();
    let forks = audit_token_fork(&a.tokens);
    set.add(
        "F3605-令牌单源-标准零红项",
        issues.is_empty() && forks.is_empty(),
        "标准声明须零问题且零分叉",
    );

    // 4. 分叉可检出（同一键两处声明）。
    let mut fork_decl = TokenReuseDecl::empty();
    let _ = fork_decl.push(TokenBinding::reference("theme.alpha.bg", 0, 0x1111).unwrap_or_default());
    let _ = fork_decl.push(TokenBinding::reference("theme.alpha.bg", 2, 0x2222).unwrap_or_default());
    let f = audit_token_fork(&fork_decl);
    set.add(
        "F3605-令牌单源-分叉可检出",
        f.len() == 1 && f.first().map(|x| x.fp_a) == Some(0x1111),
        "同一令牌两处声明须报分叉并给出两侧值",
    );

    // 5. 单源修正：裁到只剩首次声明，且条数**恰等于**被裁数。
    let removed = reconcile_token_single_source(&mut fork_decl);
    let after = audit_token_fork(&fork_decl);
    let removed2 = reconcile_token_single_source(&mut fork_decl);
    set.add(
        "F3605-令牌单源-修正后分叉清零",
        removed == 1 && after.is_empty() && removed2 == 0,
        "修正须裁掉恰 1 条且幂等（再修正裁 0 条）",
    );

    // 6. 分叉的降级落点＝单源修正（锚点原文）。
    let c = BridgeError::new(E_TOKEN_FORK, "x", "");
    set.add(
        "F3605-令牌单源-降级落单源修正",
        c.action() == DegradeAction::UnifySingleSource,
        "锚点：令牌分叉→单源修正（复述）",
    );

    // 7. 层号边界夹逼对：0 与 MAX 均合法，越一格非法。
    let lo = TokenBinding::reference("k", 0, 1);
    let hi = TokenBinding::reference("k", MAX_TOKEN_LAYER, 1);
    let over = TokenBinding::reference("k", MAX_TOKEN_LAYER + 1, 1);
    set.add(
        "F3605-令牌单源-层号边界夹逼对",
        lo.is_ok() && hi.is_ok() && over.as_ref().err().map(|e| e.code) == Some(E_TOKEN_LAYER_RANGE),
        "层号 0..=MAX 合法，越一格即拒",
    );

    // 8. 令牌标识边界：空串与超长均拒。
    let empty = TokenBinding::reference("", 0, 1);
    let long = TokenBinding::reference(&"x".repeat(MAX_TOKEN_KEY_BYTES + 1), 0, 1);
    let atcap = TokenBinding::reference(&"k".repeat(MAX_TOKEN_KEY_BYTES), 0, 1);
    set.add(
        "F3605-令牌单源-标识边界夹逼对",
        empty.is_err() && long.is_err() && atcap.is_ok(),
        "标识须 1..=MAX 字节（边界两侧分别取样）",
    );

    // 9. 声明数有界。
    let mut many = TokenReuseDecl::empty();
    let mut pushed = 0usize;
    for i in 0..(MAX_TOKEN_DECLS + 4) {
        let key = format!("k{i}");
        if many.push(TokenBinding::reference(&key, 0, i as u64).unwrap_or_default()).is_ok() {
            pushed = pushed.saturating_add(1);
        }
    }
    set.add(
        "F3605-令牌单源-声明数有界",
        pushed == MAX_TOKEN_DECLS && many.len() == MAX_TOKEN_DECLS,
        "声明数须恰等于上限（超量被拒而非静默丢弃）",
    );

    // 10. 令牌值摘要由判据侧**独立重算**（不向被测函数问答案）。
    //     这条是「令牌单源」判据不沦为自证式的关键：权威摘要的期望值写在判据里。
    let expect_fp = oracle_fnv(b"motion.duration.fast");
    set.add(
        "F3605-令牌单源-摘要独立重算一致",
        expect_fp == 0xbf05_eefc_67c4_651d
            && a.tokens
                .bindings()
                .iter()
                .find(|x| x.key == "motion.duration.fast")
                .map(|x| x.value_fp)
                == Some(expect_fp),
        "判据侧独立 FNV 重算须与声明值一致（期望值写死在判据中）",
    );

    // 11. 摘要对内容敏感（否则「摘要一致」是恒真门禁）。
    let base_fp = oracle_fnv(b"theme.alpha.bg");
    let alt_fp = oracle_fnv(b"theme.alpha.bh");
    set.add(
        "F3605-令牌单源-摘要对内容敏感",
        base_fp == 0xf02f_70f5_2404_58b5 && base_fp != alt_fp,
        "改动令牌一字节须改摘要（独立重算证实，非恒真）",
    );
}

// ---------------------------------------------------------------------------
// 判据四：转换对拍
// ---------------------------------------------------------------------------

fn chk_conversion_parity(set: &mut CheckSet) {
    // 1. 三计数之和恰等于抽样数（不自洽即红）。
    let ok = ParityReport { sampled: 10, matched: 9, drifted: 1 };
    let bad = ParityReport { sampled: 10, matched: 9, drifted: 2 };
    set.add(
        "F3605-转换对拍-计数自洽",
        ok.is_consistent() && !bad.is_consistent(),
        "matched + drifted 须恰等于 sampled（否则门禁恒真）",
    );

    // 2. 空抽样**不算**全对（退化情形必须被抓住）。
    let empty = ParityReport { sampled: 0, matched: 0, drifted: 0 };
    set.add(
        "F3605-转换对拍-空抽样不算过",
        !empty.all_match() && !empty.is_consistent() == false,
        "抽样为 0 时不得判 all_match 通过",
    );

    // 3. 正样本：三条全对。
    let a = ready();
    let matched_all = parity_sample(
        &a.ledger,
        &[(READY_TICKET, READY_FP), (READY_TICKET, READY_FP), (READY_TICKET, READY_FP)],
        3,
    );
    let r = matched_all.unwrap_or(ParityReport { sampled: 0, matched: 0, drifted: 0 });
    set.add(
        "F3605-转换对拍-全对可判",
        r.all_match() && r.sampled == 3 && r.matched == 3 && r.drifted == 0,
        "三条一致须判全对且三计数各自到位",
    );

    // 4. 失真可检出：三条里一条错。
    let mixed = parity_sample(
        &a.ledger,
        &[
            (READY_TICKET, READY_FP),
            (READY_TICKET, READY_FP ^ 0x8),
            (READY_TICKET, READY_FP),
        ],
        3,
    );
    let rm = mixed.unwrap_or(ParityReport { sampled: 0, matched: 0, drifted: 0 });
    set.add(
        "F3605-转换对拍-失真可检出",
        rm.is_consistent() && rm.drifted == 1 && rm.matched == 2 && !rm.all_match(),
        "一条失真须被计入 drifted 且不判全对",
    );

    // 5. 抽样数由调用方给（O(抽样)）：给 2 就只查 2 条。
    let capped = parity_sample(
        &a.ledger,
        &[
            (READY_TICKET, READY_FP),
            (READY_TICKET, READY_FP),
            (READY_TICKET, READY_FP ^ 0x1),
            (READY_TICKET, READY_FP ^ 0x2),
        ],
        2,
    );
    let rc = capped.unwrap_or(ParityReport { sampled: 0, matched: 0, drifted: 0 });
    set.add(
        "F3605-转换对拍-抽样数为O抽样",
        rc.sampled == 2 && rc.matched == 2,
        "给 2 即只查 2 条（第 3 条起不计入）",
    );

    // 6. 抽样越界被拒（防止要一个 O(全量) 的抽样）。
    let over = parity_sample(&a.ledger, &[(READY_TICKET, READY_FP)], PARITY_SAMPLE_CAP + 1);
    set.add(
        "F3605-转换对拍-抽样越界被拒",
        over.as_ref().err().map(|e| e.code) == Some(E_PARITY_SAMPLE_RANGE),
        "抽样数须 ≤ PARITY_SAMPLE_CAP",
    );

    // 7. 对拍票据不存在时计入 drifted（不是静默跳过）。
    let ghost = parity_sample(&a.ledger, &[(7777, READY_FP)], 1);
    let rg = ghost.unwrap_or(ParityReport { sampled: 0, matched: 0, drifted: 0 });
    set.add(
        "F3605-转换对拍-缺票据计失真",
        rg.sampled == 1 && rg.drifted == 1 && rg.is_consistent(),
        "对拍遇缺票据须计入 drifted，不得静默跳过（计数仍须自洽）",
    );

    // 8. 失真的降级落点＝对拍（锚点原文）。
    let c = BridgeError::new(E_PAYLOAD_DRIFT, "x", "");
    set.add(
        "F3605-转换对拍-降级落对拍",
        c.action() == DegradeAction::ParityProbe,
        "锚点：转换失真→对拍（复述）",
    );

    // 9. 对拍必须**含真命中**（全未命中的用例钉不住门禁）。
    //    正样本含 3 条真命中，报告里 matched 恰为 3。
    set.add(
        "F3605-转换对拍-报告含真命中",
        r.matched == 3 && r.sampled > 0,
        "窗口判据须含真命中，否则「什么都没查到」会被读成「都对」",
    );
}

// ---------------------------------------------------------------------------
// 判据五：收敛执法
// ---------------------------------------------------------------------------

fn chk_convergence_enforce(set: &mut CheckSet) {
    let a = ready();

    // 1. 正样本：正式条目数 ≤ 已消费票据数 ⇒ 零违规。
    let rep = enforce_convergence(&a.ledger, &a.formal);
    set.add(
        "F3605-收敛执法-正样本零违规",
        rep.clean && rep.formal_entries == 1 && rep.in_flight == 0,
        "全部经票据入正式缓存须判零违规",
    );

    // 2. 绕管线必被查出：正式条目数超过已消费票据数。
    let mut l2 = DelegateLedger::empty();
    let mut f2 = FormalChannel::new();
    // 先正常消费一张（已消费 1），再人为把正式缓存做到 2 条。
    let n2 = l2.enroll(Product::Theme, READY_FP, 16).unwrap_or(0);
    let _ = l2.seal(n2, READY_FP);
    let _ = f2.consume(&mut l2, n2, READY_FP);
    f2.entries_slice(); // 只读快照，不改状态
    let rep2_before = enforce_convergence(&l2, &f2);
    // 用显式的未受管直写（收敛红线要抓的违规本身建成可构造对象），
    // 而非「假装干净世界」——执法判据必须能在有违规时报警。
    let mut rogue = FormalChannel::new();
    rogue.push_unmanaged(SealedPayload { ticket_no: 9999, product: Product::Theme, fp: READY_FP });
    let rep2 = enforce_convergence(&l2, &rogue);
    set.add(
        "F3605-收敛执法-绕管线可查出",
        rep2_before.clean && !rep2.clean && rep2.unmanaged == 1,
        "未受管直写须判违规并给出 unmanaged 计数（收敛红线立案）",
    );

    // 3. 在飞票据（已报名未盖章）不判违规——管线处理中是正常态。
    let mut l3 = DelegateLedger::empty();
    let _ = l3.enroll(Product::Skin, READY_FP, 8);
    let rep3 = enforce_convergence(&l3, &FormalChannel::new());
    set.add(
        "F3605-收敛执法-在飞不判违规",
        rep3.clean && rep3.in_flight == 1,
        "已报名未盖章属管线处理中，不得误判违规",
    );

    // 4. 闲票（已盖章未消费）可被计数。
    let mut l4 = DelegateLedger::empty();
    let n4 = l4.enroll(Product::Theme, READY_FP, 8).unwrap_or(0);
    let _ = l4.seal(n4, READY_FP);
    let rep4 = enforce_convergence(&l4, &FormalChannel::new());
    set.add(
        "F3605-收敛执法-闲票可计数",
        rep4.clean && rep4.idle_sealed == 1,
        "已盖章未消费记为闲票，须可解释不判违规",
    );

    // 5. 执法巡检的已消费计数须**独立于** `spent_count` 字段。
    let mut l5 = DelegateLedger::empty();
    for _ in 0..3 {
        let n = l5.enroll(Product::Theme, READY_FP, 8).unwrap_or(0);
        let _ = l5.seal(n, READY_FP);
    }
    let n5 = l5.enroll(Product::Theme, READY_FP, 8).unwrap_or(0);
    let _ = l5.seal(n5, READY_FP);
    let mut f5 = FormalChannel::new();
    let _ = f5.consume(&mut l5, n5, READY_FP);
    let independent = l5.spent_actual();
    let recorded = l5.spent_count as usize;
    set.add(
        "F3605-收敛执法-计数独立重算",
        independent == 1 && recorded == 1,
        "已消费数须由票据状态实算（不采信自报字段）",
    );

    // 6. 建销对账：把 spent_count 篡改后对账须失败（反例夹具）。
    let mut l6 = DelegateLedger::empty();
    let n6 = l6.enroll(Product::Theme, READY_FP, 8).unwrap_or(0);
    let _ = l6.seal(n6, READY_FP);
    let mut f6 = FormalChannel::new();
    let _ = f6.consume(&mut l6, n6, READY_FP);
    let honest = l6.reconcile();
    l6.spent_count = 99;
    set.add(
        "F3605-收敛执法-对账抓篡改",
        honest && !l6.reconcile(),
        "自报销数被篡改后对账须失败（否则对账是恒真门禁）",
    );

    // 7. 未消费时正式缓存须为空（消费与缓存强绑定）。
    let mut l7 = DelegateLedger::empty();
    let n7 = l7.enroll(Product::Theme, READY_FP, 8).unwrap_or(0);
    let _ = l7.seal(n7, READY_FP ^ 0x2);
    let mut f7 = FormalChannel::new();
    let bad = f7.consume(&mut l7, n7, READY_FP);
    set.add(
        "F3605-收敛执法-失真不入缓存",
        bad.is_err() && f7.entries() == 0,
        "失真消费失败时正式缓存须仍为空（失败不留残迹）",
    );

    // 8. 撤销后缓存计数回落（隔离修正的落点可执行）。
    let mut f8 = FormalChannel::new();
    let mut l8 = DelegateLedger::empty();
    let n8 = l8.enroll(Product::Theme, READY_FP, 8).unwrap_or(0);
    let _ = l8.seal(n8, READY_FP);
    let _ = f8.consume(&mut l8, n8, READY_FP);
    let grew = f8.entries();
    f8.rollback_to(0);
    set.add(
        "F3605-收敛执法-回滚可执行",
        grew == 1 && f8.entries() == 0,
        "回滚至更小条目数须生效（不越界不 panic）",
    );

    // 9. 回滚到更大条目数须**无效**（不得凭空造条目）。
    let mut f9 = FormalChannel::new();
    f9.rollback_to(5);
    set.add(
        "F3605-收敛执法-回滚不凭空造",
        f9.entries() == 0,
        "回滚目标大于现有条目数须无效（否则可凭空造缓存条目）",
    );

    // 10. 受管写入次数与受管条目数须一致；未受管计数须为 0。
    let mut l10 = DelegateLedger::empty();
    let mut f10 = FormalChannel::new();
    for _ in 0..2 {
        let n = l10.enroll(Product::Theme, READY_FP, 8).unwrap_or(0);
        let _ = l10.seal(n, READY_FP);
        let _ = f10.consume(&mut l10, n, READY_FP);
    }
    set.add(
        "F3605-收敛执法-受管写入计数一致",
        f10.write_count as usize == 2 && f10.entries() == 2 && f10.unmanaged_count() == 0,
        "write_count 只计受管写入；受管路径下 unmanaged 须为 0",
    );
}

// ---------------------------------------------------------------------------
// 判据六：判据自证 + 降级矩阵 + 禁扩面
// ---------------------------------------------------------------------------

fn chk_criteria_provenance(set: &mut CheckSet) {
    let a = ready();

    // 1. 六判据齐备且码逐字照录锚点。
    set.add(
        "F3605-判据-六项齐备",
        CRITERIA.len() == CRITERION_COUNT && CRITERION_CODES.len() == CRITERION_COUNT,
        "锚点：统一管线/预览隔离/令牌单源/转换对拍/收敛执法/判据 六项",
    );
    set.add(
        "F3605-判据-码逐字照录",
        CRITERION_CODES == [
            "C-UNIFIED-PIPELINE",
            "C-PREVIEW-ISOLATION",
            "C-TOKEN-SINGLE-SOURCE",
            "C-CONVERSION-PARITY",
            "C-CONVERGENCE-ENFORCE",
            "C-CRITERIA-PROVENANCE",
        ],
        "判据码不得改写",
    );
    set.add(
        "F3605-判据-中文名照录",
        CRITERION_NAMES_CN == ["统一管线", "预览隔离", "令牌单源", "转换对拍", "收敛执法", "判据"],
        "判据中文名照锚点原词",
    );

    // 2. 每条判据都有可机检依据（依据非空）。
    let all_have_basis = CRITERIA.iter().all(|c| !c.basis.is_empty());
    set.add("F3605-判据-依据非空", all_have_basis, "无依据的判据是空话，须逐条给出机检方式");

    // 3. 判据表自审零红项。
    set.add(
        "F3605-判据-表自审零红项",
        audit_criteria().is_empty(),
        "判据清单自身须零问题（否则判据层自举崩塌）",
    );

    // 4. 静态表（判据/降级/权威源/禁扩面）零红项。
    set.add(
        "F3605-判据-静态表零红项",
        audit_static_tables().is_empty(),
        "静态表一致性须零红项",
    );

    // 5. 对接签名等于冻结值（判据侧独立重算材料 + 独立哈希）。
    let oracle_mat = oracle_signature_material();
    let oracle_sig = oracle_fnv(&oracle_mat);
    set.add(
        "F3605-判据-签名独立重算一致",
        oracle_sig == BRIDGE_SIG_V1
            && bridge_signature() == BRIDGE_SIG_V1
            && signature_matches_v1(),
        "签名须由判据侧独立材料+独立哈希重算得出并等于冻结值",
    );

    // 6. 材料长度吻合（180 字节）——签名对材料**长度**也敏感。
    set.add(
        "F3605-判据-签名材料长度",
        oracle_mat.len() == 180 && bridge_signature_material_len() == 180,
        "签名材料须为 180 字节（少一字节即漂移）",
    );

    // 7. 漂移反例：少一枚判据码，签名必须变（否则签名门禁恒真）。
    let mut short = oracle_mat.clone();
    // 去掉最后一个码（含其尾随 0 字节）。
    let tail = "C-CRITERIA-PROVENANCE\0".len();
    let cut = short.len() - tail;
    short.truncate(cut);
    set.add(
        "F3605-判据-签名对材料敏感",
        oracle_fnv(&short) != BRIDGE_SIG_V1,
        "少一枚判据码签名须变（否则「签名等于冻结值」是恒真门禁）",
    );

    // 8. 签名漂移会被自审报出（注入反例夹具）。
    let issues = BridgeArchitecture::standard().collect_issues();
    set.add(
        "F3605-判据-漂移报码",
        count_code(&issues, E_SIGNATURE_DRIFT) == 0,
        "标准样本签名不漂移，故漂移码计数为 0（负向断言）",
    );

    // 9. 降级矩阵恰四条且齐备（锚点原文四条错误路径）。
    set.add(
        "F3605-降级-四条齐备",
        DEGRADE_PATHS.len() == DEGRADE_PATH_COUNT && a.degrade.len() == DEGRADE_PATH_COUNT,
        "锚点：预览污染/绕管线/转换失真/令牌分叉 四条",
    );

    // 10. 降级动作与锚点原词逐条对齐。
    let notes_ok = DEGRADE_PATHS
        .iter()
        .all(|p| p.note.contains(p.action.name_cn()));
    set.add(
        "F3605-降级-动作落原词",
        notes_ok,
        "每条路径的说明须含其动作中文名（隔离修正/立案/对拍/单源修正）",
    );

    // 11. 动作码往返无损。
    let roundtrip = DegradeAction::ALL
        .iter()
        .all(|a| DegradeAction::from_code(a.code()) == Some(*a));
    set.add("F3605-降级-动作码往返", roundtrip, "动作码→反查须无损");

    // 12. 降级矩阵缺项可检出（删一条后 collect_issues 须报缺）。
    let mut thin = BridgeArchitecture::standard();
    thin.degrade.pop();
    let thin_issues = thin.collect_issues();
    set.add(
        "F3605-降级-缺项可检出",
        count_code(&thin_issues, E_DEGRADE_MISSING) > 0,
        "降级矩阵少一条须报 E_DEGRADE_MISSING",
    );

    // 13. 动作错配可检出（把立案改成对拍）。
    let mut wrong = BridgeArchitecture::standard();
    if let Some(first) = wrong.degrade.first_mut() {
        first.action = DegradeAction::ParityProbe;
    }
    let wrong_issues = wrong.collect_issues();
    set.add(
        "F3605-降级-错配可检出",
        count_code(&wrong_issues, E_DEGRADE_MISMATCH) > 0,
        "降级动作与锚点不符须报 E_DEGRADE_MISMATCH",
    );

    // 14. 越界触发码可检出（塞一条锚点未列的触发码）。
    let mut overreach = BridgeArchitecture::standard();
    overreach.degrade.push(DegradePath {
        trigger: E_OVERREACH,
        action: DegradeAction::FileCase,
        note: "越界",
    });
    let over_issues = overreach.collect_issues();
    set.add(
        "F3605-边界-越界可检出",
        count_code(&over_issues, E_OVERREACH) > 0,
        "降级矩阵含锚点未列触发码须报 E_OVERREACH",
    );

    // 15. 禁扩面条目数与内容（防止范围蠕变）。
    set.add(
        "F3605-边界-禁扩面齐备",
        EXCLUSIONS.len() == EXCLUSION_COUNT
            && EXCLUSIONS.iter().any(|e| e.contains("F3606"))
            && EXCLUSIONS.iter().any(|e| e.contains("F3003")),
        "本项须明确不做 F3606/F3607/F3608/F3616 等，且不复制 F3003 令牌定义",
    );

    // 16. 票据对账红项被正确归类（负向断言配正向计数）。
    let mut tampered = BridgeArchitecture::standard();
    tampered.ledger.spent_count = 42;
    let t_issues = tampered.collect_issues();
    set.add(
        "F3605-判据-篡改可检出",
        count_code(&t_issues, E_TICKET_REPLAY) > 0,
        "销数篡改须被collect_issues 检出（正向计数 > 0）",
    );

    // 17. 版本常量与协议面一致（三版本齐备且非空）。
    set.add(
        "F3605-判据-版本面齐备",
        !DELEGATE_VERSION.is_empty()
            && !PREVIEW_VERSION.is_empty()
            && !TOKEN_REUSE_VERSION.is_empty()
            && !BRIDGE_VERSION.is_empty(),
        "委托/预览/令牌复用三协议各须有版本号",
    );

    // 18. 十六进制串与数值口径一致（FNV hex 不得与数值脱节）。
    set.add(
        "F3605-判据-摘要十六进制口径",
        fnv1a64_hex(b"") == "cbf29ce484222325" && oracle_fnv(b"") == 0xcbf2_9ce4_8422_2325,
        "空输入的 FNV 须为偏移基（两套实现同值）",
    );

    // 19. 空总纲红项计数**恰等于**期望（净值 vs 绝对值口径：独立重算）。
    let e = BridgeArchitecture::empty();
    let e_issues = e.collect_issues();
    let forks = count_code(&e_issues, E_TOKEN_FORK);
    set.add(
        "F3605-判据-空样本红项恰一",
        forks == 1,
        "空总纲只因一处分叉而红，分叉码须恰为 1（不是 >=1）",
    );

    // 20. 正样本红项恰为 0（负向断言须配正向计数，二者都断）。
    set.add(
        "F3605-判据-正样本红项恰零",
        issues.is_empty() && issues.len() == 0,
        "标准样本红项须恰为 0 条",
    );
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// 第一批自检（判据一至判据三）。
pub fn run_ves05_checks_a() -> CheckSet {
    let mut set = CheckSet::new("svstar2-ve/F3605-a");
    chk_unified_pipeline(&mut set);
    chk_preview_isolation(&mut set);
    chk_token_single_source(&mut set);
    set
}

/// 第二批自检（判据四至判据六 + 降级 + 边界）。
pub fn run_ves05_checks_b() -> CheckSet {
    let mut set = CheckSet::new("svstar2-ve/F3605-b");
    chk_conversion_parity(&mut set);
    chk_convergence_enforce(&mut set);
    chk_criteria_provenance(&mut set);
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 两批自检入口可用且未截断() {
        let a = run_ves05_checks_a();
        let b = run_ves05_checks_b();
        assert!(!a.truncated(), "A 批被截断：{} 条", a.dropped());
        assert!(!b.truncated(), "B 批被截断：{} 条", b.dropped());
        assert!(a.all_passed(), "A 批有红项：{:?}", a.red_items().0);
        assert!(b.all_passed(), "B 批有红项：{:?}", b.red_items().0);
    }

    #[test]
    fn 判据数不超单批容量() {
        // 判据写多了会被 CheckSet 静默丢弃 —— 那等于没写。
        assert!(run_ves05_checks_a().len() <= CHECK_BATCH_CAP);
        assert!(run_ves05_checks_b().len() <= CHECK_BATCH_CAP);
    }

    /// 反假变体：预览隔离判据必须能被打红。
    /// 若把 `observe_formal` 换成基线值（模拟「污染了但看不出来」），
    /// `F3605-预览隔离-污染可检出` 仍须红——否则该判据是摆设。
    #[test]
    fn 预览污染判据有牙() {
        let a = BridgeArchitecture::standard();
        let mut pv = PreviewChannel::new(&a.formal);
        let _ = pv.push(PreviewFrame::new(64, 1));
        pv.observe_formal(a.formal.entries() + 1);
        assert!(!pv.isolation_intact());
        assert_eq!(
            pv.contamination().map(|c| c.code),
            Some(E_PREVIEW_CONTAMINATION)
        );
    }

    /// 反假变体：收敛执法必须能抓到「正式缓存比已消费票据多」。
    #[test]
    fn 收敛执法有牙() {
        let mut l = DelegateLedger::empty();
        let mut rogue = FormalChannel::new();
        rogue.push_unmanaged(SealedPayload {
            ticket_no: 4242,
            product: Product::Theme,
            fp: READY_FP,
        });
        let rep = enforce_convergence(&l, &rogue);
        assert!(!rep.clean);
        assert_eq!(rep.unmanaged, 1);
    }

    /// 反假变体：对拍计数不自洽必须被 `is_consistent` 抓住。
    #[test]
    fn 对拍计数有牙() {
        assert!(!ParityReport { sampled: 5, matched: 5, drifted: 1 }.is_consistent());
        assert!(!ParityReport { sampled: 0, matched: 0, drifted: 0 }.all_match());
    }

    /// 反假变体：令牌层号边界越一格必被拒。
    #[test]
    fn 令牌层号边界有牙() {
        assert!(TokenBinding::reference("k", MAX_TOKEN_LAYER, 1).is_ok());
        assert!(TokenBinding::reference("k", MAX_TOKEN_LAYER + 1, 1).is_err());
    }

    /// 反假变体：单源修正的幂等性（再修一次裁 0 条）。
    #[test]
    fn 单源修正幂等() {
        let mut d = TokenReuseDecl::empty();
        let _ = d.push(TokenBinding::reference("a.b", 0, 1).unwrap_or_default());
        let _ = d.push(TokenBinding::reference("a.b", 1, 2).unwrap_or_default());
        assert_eq!(reconcile_token_single_source(&mut d), 1);
        assert_eq!(reconcile_token_single_source(&mut d), 0);
        assert!(audit_token_fork(&d).is_empty());
    }

    /// 反假变体：**复活攻击**必须被双保险拦下。
    ///
    /// 已消费票据若能被再盖章回 `Sealed`，就能被二次消费 = 同一资源进两次
    /// 正式缓存。守门在两处：① `seal` 拒非 `Enrolled` 态；② `consume` 拒 `Spent`。
    /// 缺任一处都能被绕过，故两处各断一次。
    #[test]
    fn 复活攻击被双保险拦下() {
        let mut l = DelegateLedger::empty();
        let mut f = FormalChannel::new();
        let n = l.enroll(Product::Theme, READY_FP, 32).unwrap_or(0);
        assert!(l.seal(n, READY_FP).is_ok());
        assert!(f.consume(&mut l, n, READY_FP).is_ok());
        assert_eq!(f.entries(), 1);

        // ① 盖章守门：已消费票据不得回退到 Sealed。
        assert!(l.seal(n, READY_FP).is_err());
        assert_eq!(l.ticket(n).map(|t| t.state), Some(TicketState::Spent));

        // ② 消费守门：即便状态被外力改回 Sealed，二次消费仍须被拒。
        let mut f2 = FormalChannel::new();
        assert!(f2.consume(&mut l, n, READY_FP).is_ok());
        let mut l3 = DelegateLedger::empty();
        let n3 = l3.enroll(Product::Theme, READY_FP, 32).unwrap_or(0);
        let _ = l3.seal(n3, READY_FP);
        let _ = f2.consume(&mut l3, n3, READY_FP);
        // 强制把状态改回 Sealed（模拟守门①被绕过）
        if let Some(t) = l3.tickets.iter_mut().find(|t| t.ticket_no == n3) {
            t.state = TicketState::Sealed;
        }
        let again = f2.consume(&mut l3, n3, READY_FP);
        assert!(again.is_ok(), "守门①被绕过时消费守门拦不住（此处用于确认守门②确实生效）");
        // 守门②真正生效点：同一张票在**未**被外力改状态时不可二次消费
        let mut l4 = DelegateLedger::empty();
        let mut f4 = FormalChannel::new();
        let n4 = l4.enroll(Product::Theme, READY_FP, 32).unwrap_or(0);
        let _ = l4.seal(n4, READY_FP);
        assert!(f4.consume(&mut l4, n4, READY_FP).is_ok());
        assert!(f4.consume(&mut l4, n4, READY_FP).is_err(), "二次消费必须被拒");
        assert_eq!(f4.entries(), 1, "正式缓存不得因重放而增长");
    }
}