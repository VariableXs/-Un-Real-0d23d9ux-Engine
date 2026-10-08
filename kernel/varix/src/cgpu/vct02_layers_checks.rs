//! vct02_layers_checks —— CGPU-F3042 对接协议分层模型 · 域自检（判据逐条映射，六组+META 16 项）
//!
//! 锚点判据 → 判据组：
//! - 分层表（三层闭集逐行在册）→ [`group_table`]
//! - 传输层（帧封装与校验）→ [`group_tx`]
//! - 会话层（会话上下文与序号）→ [`group_ses`]
//! - 语义层（消息类型与负载）→ [`group_sem`]
//! - 协议栈（三层贯通往返）→ [`group_stack`]
//! - 码段（独占段+守门）→ [`group_meta`]
//! - 判据（收口自检）→ [`group_meta2`]
//!
//! 双向验证纪律：合法帧先证可解再翻位（恒红校验不行）；重放必拒但
//! 正常三连收全通（恒拒通道不行）；上限负载先证可编再超限（恒假门禁不行）。

use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::cgpu::vct01_tbridge::Segment;
use crate::cgpu::vct02_layers::*;

/// 判据入口（聚合器经 mod.rs 调用）。
pub fn run_vct02_checks() -> CheckSet {
    let mut set = CheckSet::new("CGPU-F3042");
    group_table(&mut set);
    group_tx(&mut set);
    group_ses(&mut set);
    group_sem(&mut set);
    group_stack(&mut set);
    group_meta(&mut set);
    group_meta2(&mut set);
    set
}

// ---------------------------------------------------------------------------
// 分层表
// ---------------------------------------------------------------------------

fn group_table(set: &mut CheckSet) {
    // ① 三层闭集：层号 1/2/3 与索引一一对应，from_index 闭集外拒绝。
    let mut nos_ok = LAYER_TABLE.len() == 3;
    let mut i = 0usize;
    while i < LAYER_TABLE.len() {
        if LAYER_TABLE[i].no != (i + 1) as u8 {
            nos_ok = false;
        }
        i += 1;
    }
    let idx_ok = nos_ok
        && matches!(LayerId::from_index(0), Some(LayerId::Transport))
        && matches!(LayerId::from_index(1), Some(LayerId::Session))
        && matches!(LayerId::from_index(2), Some(LayerId::Semantic))
        && LayerId::from_index(3).is_none();
    set.add(
        "C3042-LAYTBL-01 三层闭集层号递增且索引边界拒绝",
        idx_ok,
        "传输/会话/语义三层自下而上编号；闭集之外不猜测（没有第四层）",
    );

    // ② 三层名逐字在册且枚举口径同源互异。
    let expect = ["传输层", "会话层", "语义层"];
    let mut names_match = LAYER_TABLE.len() == expect.len();
    let mut i = 0usize;
    while i < LAYER_TABLE.len() && i < expect.len() {
        if let Some(row) = LAYER_TABLE.get(i) {
            if row.name != expect[i] {
                names_match = false;
            }
        } else {
            names_match = false;
        }
        i += 1;
    }
    let names_ok = names_match
        && LayerId::Transport.name() == "传输层"
        && LayerId::Session.name() == "会话层"
        && LayerId::Semantic.name() == "语义层"
        && LayerId::Transport.name() != LayerId::Session.name()
        && LayerId::Session.name() != LayerId::Semantic.name();
    set.add(
        "C3042-LAYTBL-02 三层名逐字在册且枚举同源互异",
        names_ok,
        "锚点原文「传输层/会话层/语义层三层」逐字可 grep；表与枚举双口径同源",
    );

    // ③ 分层表对账：开销合计=栈总开销 + 职责句逐条非空独立。
    let mut duty_ok = true;
    let mut i = 0usize;
    while i < LAYER_TABLE.len() {
        if LAYER_TABLE[i].duty.is_empty() {
            duty_ok = false;
        }
        let mut j = i + 1;
        while j < LAYER_TABLE.len() {
            if LAYER_TABLE[i].duty == LAYER_TABLE[j].duty {
                duty_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    let overhead_ok = TRANSPORT_OVERHEAD == 3
        && SESSION_OVERHEAD == 6
        && SEMANTIC_OVERHEAD == 1
        && STACK_OVERHEAD_TOTAL == 10
        && layer_audit();
    set.add(
        "C3042-LAYTBL-03 分层表开销对账且职责句逐条独立",
        overhead_ok && duty_ok,
        "3+6+1=10 分层不是感觉是对账面；空句=职责缺失、重复句=层责混淆",
    );
}

// ---------------------------------------------------------------------------
// 传输层
// ---------------------------------------------------------------------------

fn group_tx(set: &mut CheckSet) {
    // ④ 合法帧编解码往返相等（非平凡负载逐字节）。
    let payload: [u8; 8] = [0xA1, 0xB2, 0xC3, 0xD4, 0x00, 0xFF, 0x5D, 0x7E];
    let round_ok = match transport_frame(&payload) {
        Ok(f) => match transport_unframe(&f) {
            Ok(back) => back == payload && f.len() == payload.len() + TRANSPORT_OVERHEAD as usize,
            Err(_) => false,
        },
        Err(_) => false,
    };
    set.add(
        "C3042-TX-01 帧编解码逐字节往返相等",
        round_ok,
        "传输层=帧封装与校验：往返还原是可靠搬运的底线",
    );

    // ⑤ 校验位翻转必拒——先证基线帧可解（非恒红），再翻 payload 字节。
    let flip_ok = match transport_frame(&payload) {
        Ok(mut f) => {
            let baseline = matches!(transport_unframe(&f), Ok(ref b) if *b == payload);
            let mid = TRANSPORT_OVERHEAD as usize + 3;
            let flipped = match f.get_mut(mid) {
                Some(byte) => {
                    *byte ^= 0x01;
                    true
                }
                None => false,
            };
            flipped && baseline && matches!(transport_unframe(&f), Err(LvCode::TX_CHECKSUM))
        }
        Err(_) => false,
    };
    set.add(
        "C3042-TX-02 单字节翻转必拒且基线可解",
        flip_ok,
        "校验位对任一 payload 字节翻转敏感：链路损坏在传输层显性失败不猜内容",
    );

    // ⑥ 帧长非法三路：空帧、长度字段不符、负载超限——上限先证可编。
    let max_ok = matches!(transport_frame(&[0u8; 252]), Ok(_));
    let empty_r = transport_frame(&[]);
    let len_r = transport_unframe(&[0x03, 0x5D, 0x5D, 0xAA, 0xBB]);
    let mut over = Vec::new();
    let mut n = 0usize;
    while n <= MAX_TRANSPORT_PAYLOAD {
        over.push(0u8);
        n += 1;
    }
    let over_r = transport_frame(&over);
    let len_ok = max_ok
        && matches!(empty_r, Err(LvCode::TX_LENGTH))
        && matches!(len_r, Err(LvCode::TX_LENGTH))
        && matches!(over_r, Err(LvCode::TX_LENGTH));
    set.add(
        "C3042-TX-03 空帧/长度不符/超限三路显性拒绝",
        len_ok,
        "上限 252 先证可编再测 253 超限（非恒假门禁）；空帧没有语义出不了发送端",
    );
}

// ---------------------------------------------------------------------------
// 会话层
// ---------------------------------------------------------------------------

fn group_ses(set: &mut CheckSet) {
    // ⑦ 串户必拒：A 会话的字节喂给 B 通道 → 0x5D03；零会话开通道拒绝。
    let mut a = match SessionChannel::new(0x1111_2222) {
        Ok(s) => s,
        Err(_) => return,
    };
    let a_bytes = match a.wrap(&[0x01, 0x02]) {
        Ok(b) => b,
        Err(_) => return,
    };
    let mut b = match SessionChannel::new(0x3333_4444) {
        Ok(s) => s,
        Err(_) => return,
    };
    let cross_r = b.unwrap(&a_bytes);
    let zero_r = SessionChannel::new(0);
    let cross_ok = matches!(cross_r, Err(LvCode::SES_UNKNOWN)) && matches!(zero_r, Err(LvCode::SES_UNKNOWN));
    set.add(
        "C3042-SES-01 会话不匹配串户必拒且零会话拒绝",
        cross_ok,
        "S10 衔接包约定协议承载面：串户在会话层显性失败——S域隔离不串户的分层延续",
    );

    // ⑧ 重放必拒：同帧二次解封 seq 非递增 → 0x5D04（首解封先证合法）。
    let mut c = match SessionChannel::new(0x5555_6666) {
        Ok(s) => s,
        Err(_) => return,
    };
    let frame1 = match c.wrap(&[0x7E]) {
        Ok(b) => b,
        Err(_) => return,
    };
    let first_ok = matches!(c.unwrap(&frame1), Ok(_));
    let replay_r = c.unwrap(&frame1);
    let replay_ok = first_ok && matches!(replay_r, Err(LvCode::SES_SEQ));
    set.add(
        "C3042-SES-02 重放帧必拒且首收先证合法",
        replay_ok,
        "接收流严格递增：重放与乱序在会话层显性失败；首收合法证明通道非恒拒",
    );

    // ⑨ 正常三连收全通且序号推进 1/2/3（恒拒通道的反证）。
    let mut d = match SessionChannel::new(0x7777_8888) {
        Ok(s) => s,
        Err(_) => return,
    };
    let w1 = d.wrap(&[0x01]);
    let w2 = d.wrap(&[0x02]);
    let w3 = d.wrap(&[0x03]);
    let mut frames = Vec::new();
    frames.push(w1);
    frames.push(w2);
    frames.push(w3);
    let mut seq_ok = true;
    let mut k = 0usize;
    while k < frames.len() {
        let frame = match frames[k] {
            Ok(ref f) => f.clone(),
            Err(_) => {
                seq_ok = false;
                break;
            }
        };
        match d.unwrap(&frame) {
            Ok(_) => {}
            Err(_) => {
                seq_ok = false;
                break;
            }
        }
        k += 1;
    }
    let advanced = d.tx_seq == 3 && d.rx_seq == 3;
    set.add(
        "C3042-SES-03 三连收发全通且双流序号推进一致",
        seq_ok && advanced,
        "tx/rx 双流各自严格递增：发送 1/2/3 对接收 1/2/3——通道活着且账实相符",
    );
}

// ---------------------------------------------------------------------------
// 语义层
// ---------------------------------------------------------------------------

fn group_sem(set: &mut CheckSet) {
    // ⑩ 四 kind 编解码逐字往返 + 与 T01 四段索引逐一对拍。
    let kinds = [SEM_KIND_COMMAND, SEM_KIND_STATE, SEM_KIND_ASSET, SEM_KIND_EVENT];
    let mut round_ok = true;
    let mut seg_ok = true;
    let mut k = 0usize;
    while k < kinds.len() {
        match semantic_encode(kinds[k], &[0x11, 0x22]) {
            Ok(b) => match semantic_decode(&b) {
                Ok(m) => {
                    if m.kind != kinds[k] || m.payload != [0x11, 0x22] {
                        round_ok = false;
                    }
                }
                Err(_) => round_ok = false,
            },
            Err(_) => round_ok = false,
        }
        match (kind_to_segment_index(kinds[k]), Segment::from_index(k as u8)) {
            (Some(si), Some(seg)) => {
                if si != k as u8 {
                    seg_ok = false;
                }
                let seg_idx_ok = match seg {
                    Segment::Command => si == 0,
                    Segment::State => si == 1,
                    Segment::Asset => si == 2,
                    Segment::Event => si == 3,
                };
                if !seg_idx_ok {
                    seg_ok = false;
                }
            }
            _ => seg_ok = false,
        }
        k += 1;
    }
    set.add(
        "C3042-SEM-01 四段语义往返相等且与 T01 四段索引对拍",
        round_ok && seg_ok,
        "语义闭集 1..=4 对应命令/状态/资产/事件四段——分层模型与 F3041 开工口径衔接",
    );

    // ⑪ 未知类型双端拒绝：编码端 0/5 拒；解码端空帧/未知类型拒。
    let enc0 = semantic_encode(0, &[0x01]);
    let enc5 = semantic_encode(5, &[0x01]);
    let dec_empty = semantic_decode(&[]);
    let dec_unknown = semantic_decode(&[0x09, 0x01]);
    let unknown_ok = matches!(enc0, Err(LvCode::SEM_KIND))
        && matches!(enc5, Err(LvCode::SEM_KIND))
        && matches!(dec_empty, Err(LvCode::SEM_KIND))
        && matches!(dec_unknown, Err(LvCode::SEM_KIND));
    set.add(
        "C3042-SEM-02 未知语义类型双端显性拒绝",
        unknown_ok,
        "未知类型在编码端就显性失败不等链路对端兜圈子；闭集之外没有第五种语义",
    );
}

// ---------------------------------------------------------------------------
// 协议栈
// ---------------------------------------------------------------------------

fn group_stack(set: &mut CheckSet) {
    // ⑫ 全链路往返：三层加封 10 字节 → 三层剥净原样还原。
    let mut st = match ProtocolStack::new(0x0B0E_0001) {
        Ok(s) => s,
        Err(_) => return,
    };
    let payload: [u8; 6] = [0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F];
    let round_ok = match st.stack_send(SEM_KIND_STATE, &payload) {
        Ok(frame) => {
            let len_ok = frame.len() == payload.len() + STACK_OVERHEAD_TOTAL as usize;
            match st.stack_recv(&frame) {
                Ok(msg) => len_ok && msg.kind == SEM_KIND_STATE && msg.payload == payload,
                Err(_) => false,
            }
        }
        Err(_) => false,
    };
    set.add(
        "C3042-STACK-01 全链路三层往返逐字节还原",
        round_ok,
        "自上而下封装自下而上剥净：帧长=payload+10 证明三层封套恰各在位",
    );

    // ⑬ 栈级审计为真 + 语义超限在入口拒绝（上限先证可发）。
    let audit_true = stack_audit();
    let boundary = match ProtocolStack::new(0x0B0E_0002) {
        Ok(mut s) => matches!(s.stack_send(SEM_KIND_COMMAND, &[0u8; MAX_SEMANTIC_PAYLOAD]), Ok(_)),
        Err(_) => false,
    };
    let mut over = Vec::new();
    let mut n = 0usize;
    while n <= MAX_SEMANTIC_PAYLOAD {
        over.push(0u8);
        n += 1;
    }
    let over_r = match ProtocolStack::new(0x0B0E_0003) {
        Ok(mut s) => matches!(s.stack_send(SEM_KIND_COMMAND, &over), Err(LvCode::TX_LENGTH)),
        Err(_) => false,
    };
    set.add(
        "C3042-STACK-02 栈级审计为真且语义超限入口拒绝",
        audit_true && boundary && over_r,
        "审计含分层表对账+四段对拍+真实往返；上限先证可发再测超限（非恒假门禁）",
    );
}

// ---------------------------------------------------------------------------
// 码段与判据收口
// ---------------------------------------------------------------------------

fn group_meta(set: &mut CheckSet) {
    // ⑭ 码段独占：0x5D 高字节（0x5A=vct01、0x5B=vcv01、0x5C=vcw01 已占）
    // + 五码互异。
    set.add(
        "C3042-META-00 码段独占 0x5D",
        LvCode::TX_CHECKSUM.code() & 0xFF00 == 0x5D00
            && LvCode::TX_LENGTH.code() & 0xFF00 == 0x5D00
            && LvCode::SES_UNKNOWN.code() & 0xFF00 == 0x5D00
            && LvCode::SES_SEQ.code() & 0xFF00 == 0x5D00
            && LvCode::SEM_KIND.code() & 0xFF00 == 0x5D00
            && LvCode::TX_CHECKSUM.code() != LvCode::TX_LENGTH.code()
            && LvCode::SES_UNKNOWN.code() != LvCode::SES_SEQ.code()
            && LvCode::SEM_KIND.code() != LvCode::TX_CHECKSUM.code()
            && LvCode::SES_SEQ.code() != LvCode::SEM_KIND.code(),
        "码段互异判据用 != 防自判死；逐层独占=错误停在发生它的那一层",
    );
}

fn group_meta2(set: &mut CheckSet) {
    // ⑮ 实挂条数从 CheckSet 实取：前六组 14 条，META2 段 2 条，合计 16。
    let before = set.len();
    set.add(
        "C3042-META-01 实挂条数+2(META2)=声明条数16",
        before == 14 && before + 2 == 16,
        "实 add 数从 CheckSet.len() 实取；增删判据漏改口径即红",
    );

    // ⑯ 判据名全集互异。
    let mut names: Vec<&'static str> = Vec::new();
    let mut k = 0usize;
    while k < set.len() {
        if let Some(ch) = set.get(k) {
            names.push(ch.name);
        }
        k += 1;
    }
    let mut all_differ = true;
    let mut i = 0usize;
    while i < names.len() {
        let mut j = i + 1;
        while j < names.len() {
            if names[i] == names[j] {
                all_differ = false;
            }
            j += 1;
        }
        i += 1;
    }
    set.add(
        "C3042-META-02 判据名全集互异",
        all_differ && set.len() + 1 == 16,
        "重名判据让 tally 与实际脱节——收口时逐名实取对拍",
    );
}
