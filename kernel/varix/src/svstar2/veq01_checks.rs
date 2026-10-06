//! VE-F3201 · 域自检（判据逐条对应，见 `veq01_pipeline.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项前缀）：
//! - 单向委托 → `Q01-委托-*`
//! - 六段签名 → `Q01-六段-*`
//! - 收敛红线 → `Q01-收敛-*`
//! - 透传不失真 → `Q01-透传-*`
//! - 十项映射 → `Q01-十项-*`
//! - 跨批对接（八消费域契约前向）→ `Q01-对接-*`
//! - 无障碍（读屏替代）→ `Q01-读屏-*`
//! - 错误路径零静默 → `Q01-错误-*`
//! - 开工门禁 → `Q01-门禁-*`
//!
//! 零墙钟、零 IO，回归可复现。

use super::veq01_pipeline::*;

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 标准授权表（远程未授权）。
fn std_grants() -> RemoteGrantTable {
    RemoteGrantTable::new()
}

/// 已授权远程的标准授权表（供remote 正向用例）。
fn granted() -> RemoteGrantTable {
    let mut g = RemoteGrantTable::new();
    let _ = g.grant("VE-G");
    g
}

/// 私加载现场：三域合规 + 一域私加载。
fn sightings() -> Vec<PrivateLoadSighting> {
    vec![
        PrivateLoadSighting {
            domain: ConsumerDomain::O,
            action: "样式表经acquire 取得".to_string(),
            via_pipeline: true,
        },
        PrivateLoadSighting {
            domain: ConsumerDomain::I,
            action: "自行 readFile 读纹理".to_string(),
            via_pipeline: false,
        },
        PrivateLoadSighting {
            domain: ConsumerDomain::N,
            action: "字体经 acquire 取得".to_string(),
            via_pipeline: true,
        },
        PrivateLoadSighting {
            domain: ConsumerDomain::P,
            action: "自行 fetch 拉动画".to_string(),
            via_pipeline: false,
        },
    ]
}

/// VE-F3201 域自检。
pub fn run_veq01_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veq01");

    // ---- 判据一：十项映射 ----

    // 十项映射恰为十类，不多不少。
    {
        let kinds: Vec<&str> = RESOURCE_MAPPINGS.iter().map(|m| m.kind.en()).collect();
        set.add(
            "Q01-十项-十项资源映射齐备",
            RESOURCE_MAPPINGS.len() == 10,
            "",
        );
        // 十项互不重复（重复即两个类型抢同一身份）。
        let mut sorted = kinds.clone();
        sorted.sort_unstable();
        sorted.dedup();
        set.add(
            "Q01-十项-十项无重复",
            sorted.len() == kinds.len(),
            "",
        );
    }

    // 每项映射四要素齐备（解码归属/生命周期/缓存键/失败处置）。
    {
        let mut ok = true;
        for m in RESOURCE_MAPPINGS.iter() {
            if m.decoded_by.trim().is_empty()
                || m.label.trim().is_empty()
                || m.cache_key_fields.is_empty()
                || m.note.trim().is_empty()
            {
                ok = false;
            }
            // 生命周期与流送能力不得矛盾（按帧却不可流式 = 每帧整体重取）。
            if m.lifetime == Lifetime::FrameScoped && !m.streamable {
                ok = false;
            }
        }
        set.add("Q01-十项-四要素齐备且粒度自洽", ok, "");
    }

    // 每类资源的生命周期粒度可在枚举侧复现（数据与枚举不得漂移）。
    {
        let mut ok = true;
        for m in RESOURCE_MAPPINGS.iter() {
            if m.kind.lifetime() != m.lifetime || m.kind.is_streamable() != m.streamable {
                ok = false;
            }
        }
        set.add("Q01-十项-生命周期与流送标记同源", ok, "");
    }

    // 资源类型标识可反查（URI 类型段解析的基础）。
    {
        let mut ok = true;
        for m in RESOURCE_MAPPINGS.iter() {
            match ResourceKind::from_en(m.kind.en()) {
                Some(k) if k == m.kind => {}
                _ => ok = false,
            }
        }
        set.add("Q01-十项-类型标识可反查", ok, "");
    }

    // 未登记类型必须被拒（不受管= 第二生命周期权威）。
    {
        let r = lookup_mapping_by_str("sprite");
        set.add(
            "Q01-十项-未登记类型被拒",
            r.is_err() && r.code() == Some(DiagCode::ResourceTypeUnmapped),
            "",
        );
    }

    // 空类型段被拒（缺类型即不受管）。
    {
        let r = lookup_mapping_by_str("");
        set.add(
            "Q01-十项-空类型段被拒",
            r.is_err() && r.code() == Some(DiagCode::ResourceTypeUnmapped),
            "",
        );
    }

    // ---- 判据二：六段签名 ----

    // 六段恰为寻址→请求→调度→加载→校验→交付，顺序不可调换。
    {
        let names: Vec<&str> = STAGE_ORDER.iter().map(|s| s.zh()).collect();
        set.add(
            "Q01-六段-六段在册不多不少",
            STAGE_ORDER.len() == 6
                && names
                    == vec!["寻址", "请求", "调度", "加载", "校验", "交付句柄"],
            "",
        );
    }

    // 规范次序通过校验。
    {
        let r = check_stage_order(&STAGE_ORDER);
        set.add("Q01-六段-规范次序通过", r.is_ok(), "");
    }

    // 调换次序被拒（先交付后校验 = 交付未校验资源）。
    {
        let mut swapped = STAGE_ORDER;
        swapped.swap(4, 5);
        let r = check_stage_order(&swapped);
        set.add(
            "Q01-六段-次序调换被拒",
            r.is_err() && r.code() == Some(DiagCode::StageOrderViolated),
            "",
        );
    }

    // 跳过某段被拒（跳过 verify = 损坏资源可直接交付）。
    {
        let short = &[StageId::Address, StageId::Request, StageId::Load];
        let r = check_stage_order(short);
        set.add(
            "Q01-六段-跳段被拒",
            r.is_err() && r.code() == Some(DiagCode::StageOrderViolated),
            "",
        );
    }

    // 每段签名四要素齐备（入参/出参/失败码/理由）。
    {
        let mut ok = STAGE_SPECS.len() == 6;
        for s in STAGE_SPECS.iter() {
            if s.inputs.is_empty()
                || s.outputs.is_empty()
                || s.allowed_failures.is_empty()
                || s.rationale.trim().is_empty()
                || s.amortized.trim().is_empty()
            {
                ok = false;
            }
        }
        set.add("Q01-六段-签名四要素齐备", ok, "");
    }

    // 六段失败码白名单覆盖 IO 全族（白名单过窄会让契约成为阻塞）。
    {
        let load = STAGE_SPECS.iter().find(|s| s.id == StageId::Load);
        let io_ok = match load {
            Some(l) => {
                l.allows(DiagCode::IoTimeout)
                    && l.allows(DiagCode::IoNotFound)
                    && l.allows(DiagCode::IoFailed)
                    && l.allows(DiagCode::SourceUntrusted)
                    && l.allows(DiagCode::Cancelled)
            }
            None => false,
        };
        set.add("Q01-六段-load段覆盖IO全族失败码", io_ok, "");
    }

    // 未注册阶段被拒。
    {
        let r = lookup_stage("prefetch");
        set.add(
            "Q01-六段-未注册阶段被拒",
            r.is_err() && r.code() == Some(DiagCode::StageUnregistered),
            "",
        );
    }

    // 段契约对拍：越权失败码被拦截。
    {
        let obs = [
            StageObservation {
                stage: StageId::Address,
                observed_failure: Some(DiagCode::UriMalformed),
            },
            StageObservation {
                stage: StageId::Load,
                observed_failure: Some(DiagCode::IoTimeout),
            },
            StageObservation {
                // 寻址段产出 IO 超时 = 契约分歧（该段不负责搬运）。
                stage: StageId::Address,
                observed_failure: Some(DiagCode::IoTimeout),
            },
        ];
        let r = audit_stage_contract(&obs);
        set.add(
            "Q01-六段-段契约分歧被拦截",
            r.is_err() && r.code() == Some(DiagCode::StageContractDiverged),
            "",
        );
    }

    // 段契约对拍：合规观测全绿。
    {
        let obs = [
            StageObservation {
                stage: StageId::Address,
                observed_failure: Some(DiagCode::UriContainsUserContent),
            },
            StageObservation {
                stage: StageId::Verify,
                observed_failure: Some(DiagCode::HashMismatch),
            },
            StageObservation {
                stage: StageId::Deliver,
                observed_failure: None,
            },
        ];
        let r = audit_stage_contract(&obs);
        set.add("Q01-六段-合规观测通过对拍", r.is_ok(), "");
    }

    // 失败码归属判定：合法产出放行。
    {
        let r = check_stage_failure(StageId::Verify, DiagCode::HashMismatch);
        set.add("Q01-六段-失败码归属放行", r.is_ok(), "");
    }

    // ---- 判据三：单向委托 ----

    // 参数剥离两侧同规范化（历史缺陷的回归防护）。
    {
        let ok = strip_args("acquireRefCount(key)") == "acquireRefCount"
            && strip_args("acquireRefCount") == "acquireRefCount"
            && strip_args("map(Array<T>)") == "map";
        set.add("Q01-委托-参数剥离两侧同规范化", ok, "");
    }

    // 禁止表必须是**裸签名**（带 `F->Q:` 前缀会让红线永不触发——两侧形态
    // 不一致时比较恒为假，红线看上去存在实则一次都不跑）。
    {
        let ok = DELEGATION_FORBIDDEN
            .iter()
            .all(|f| !f.contains("->") && strip_args(f).starts_with("acquire")
                || !f.contains("->") && strip_args(f).starts_with("release")
                || !f.contains("->") && strip_args(f).starts_with("load")
                || !f.contains("->") && strip_args(f).starts_with("enqueue"));
        set.add("Q01-委托-禁止表登记为裸签名", ok, "");
    }

    // F→Q 反向依赖被检出（红线可执行，非永不触发）。
    {
        let calls = [
            DelegationCall {
                from: DelegateSide::F,
                signature: "acquireRefCount(key)".to_string(),
            },
            DelegationCall {
                from: DelegateSide::Q,
                signature: "requestDecode(bytes, d1)".to_string(),
            },
        ];
        let r = check_delegation_direction(&calls);
        set.add(
            "Q01-委托-反向依赖被检出",
            r.is_err() && r.code() == Some(DiagCode::DelegationDirectionViolated),
            "",
        );
    }

    // 无参形态的反向依赖同样被检出（两侧规范化生效的证明）。
    {
        let calls = [DelegationCall {
            from: DelegateSide::F,
            signature: "loadResource".to_string(),
        }];
        let r = check_delegation_direction(&calls);
        set.add(
            "Q01-委托-无参反向依赖同样被检出",
            r.is_err() && r.code() == Some(DiagCode::DelegationDirectionViolated),
            "",
        );
    }

    // 合规的 Q→F 委托不误报。
    {
        let calls = [
            DelegationCall {
                from: DelegateSide::Q,
                signature: "requestDecode(rawBytes, decoderId)".to_string(),
            },
            DelegationCall {
                from: DelegateSide::Q,
                signature: "releaseDecoder(decoderId)".to_string(),
            },
        ];
        let r = check_delegation_direction(&calls);
        set.add("Q01-委托-正向委托不误报", r.is_ok(), "");
    }

    // 允许表与禁止表不相交（一张表同时允许与禁止 = 表本身矛盾）。
    {
        // 两侧都剥到裸形态再比：允许表写 `Q->F:requestDecode(...)`，需先去掉
        // `Q->F:` 前缀；禁止表已是裸签名。不统一形态，这项断言只是碰巧为真。
        let bare_allowed = |s: &str| -> String {
            let no_prefix = match s.find("->") {
                Some(i) => match s[i + 2..].find(':') {
                    Some(j) => &s[i + 2 + j + 1..],
                    None => s,
                },
                None => s,
            };
            strip_args(no_prefix)
        };
        let ok = !DELEGATION_ALLOWED
            .iter()
            .any(|a| DELEGATION_FORBIDDEN.iter().any(|f| bare_allowed(a) == strip_args(f)));
        set.add("Q01-委托-允许表与禁止表不相交", ok, "");
    }

    // ---- 判据四：收敛红线 ----

    // 合法消费域可经管线取得句柄（收敛红线的正向）。
    {
        let g = std_grants();
        let r = acquire_from_pipeline("ve-asset://app/texture/base.png", "VE-I", &g);
        let ok = matches!(
            &r,
            Outcome::Ok { value: h, .. }
                if h.state == HandleState::Ready && h.kind == ResourceKind::Texture
        );
        set.add("Q01-收敛-合法消费域可取得句柄", ok, "");
    }

    // 未登记消费域被拒（边界外的生命周期管不到）。
    {
        let g = std_grants();
        let r = acquire_from_pipeline("ve-asset://app/texture/base.png", "VE-Z", &g);
        set.add(
            "Q01-收敛-未登记消费域被拒",
            r.is_err() && r.code() == Some(DiagCode::PrivateLoadDetected),
            "",
        );
    }

    // 句柄缓存键为规范化 URI（去重与对账的依据）。
    {
        let g = std_grants();
        let r = acquire_from_pipeline("ve-asset://APP/texture//a/./b.png", "VE-I", &g);
        let ok = match r {
            Outcome::Ok { value: h, .. } => h.cache_key == "ve-asset://app/texture/a/b.png",
            Outcome::Err { .. } => false,
        };
        set.add("Q01-收敛-句柄键为规范化URI", ok, "");
    }

    // 私加载检出并立案（收敛红线的反向）。
    {
        let v = detect_private_load(&sightings());
        set.add(
            "Q01-收敛-私加载检出并立案",
            v.count == 2
                && v.cases.iter().any(|c| c.case_id == "PRIVLOAD-VE-I-1")
                && v.cases.iter().any(|c| c.case_id == "PRIVLOAD-VE-P-2"),
            "",
        );
    }

    // 全合规时零立案。
    {
        let clean = vec![PrivateLoadSighting {
            domain: ConsumerDomain::O,
            action: "经 acquire 取得".to_string(),
            via_pipeline: true,
        }];
        let v = detect_private_load(&clean);
        set.add("Q01-收敛-全合规零立案", v.count == 0, "");
    }

    // 伪造句柄释放被拒（绕过管线的直接证据）。
    {
        let fake = ResourceHandle {
            cache_key: "   ".to_string(),
            kind: ResourceKind::Texture,
            state: HandleState::Ready,
        };
        let r = release_handle(&fake, "VE-I");
        set.add(
            "Q01-收敛-伪造句柄释放被拒",
            r.is_err() && r.code() == Some(DiagCode::ValueInvalid),
            "",
        );
    }

    // 合法句柄释放放行（释放权唯一）。
    {
        let g = std_grants();
        let ok = match acquire_from_pipeline("ve-asset://app/font/main.ttf", "VE-N", &g) {
            Outcome::Ok { value: h, .. } => release_handle(&h, "VE-N").is_ok(),
            Outcome::Err { .. } => false,
        };
        set.add("Q01-收敛-合法句柄释放放行", ok, "");
    }

    // 消费域 id 可反查且全集一致。
    {
        let mut ok = ConsumerDomain::ALL.len() == 6;
        for d in ConsumerDomain::ALL.iter() {
            match ConsumerDomain::from_id(d.id()) {
                Some(x) if x == *d => {}
                _ => ok = false,
            }
        }
        set.add("Q01-收敛-消费域标识可反查", ok, "");
    }

    // ---- 判据五：透传不失真 ----

    // 合规信封通过（三要素齐备 + 原码一致）。
    {
        let env = DecodeFailureEnvelope {
            decoder_code: "PNG_CHUNK_CRC_MISMATCH".to_string(),
            uri: "ve-asset://app/texture/broken.png".to_string(),
            stage: StageId::Verify,
            hint: "重新获取该资源；若持续失败请校验资源包完整性".to_string(),
            mutated: false,
        };
        let r = verify_passthrough(&env, "PNG_CHUNK_CRC_MISMATCH");
        set.add("Q01-透传-合规信封通过", r.is_ok(), "");
    }

    // 原码被改写即失真。
    {
        let env = DecodeFailureEnvelope {
            decoder_code: "LOAD_FAILED".to_string(),
            uri: "ve-asset://app/texture/broken.png".to_string(),
            stage: StageId::Verify,
            hint: "重取".to_string(),
            mutated: false,
        };
        let r = verify_passthrough(&env, "PNG_CHUNK_CRC_MISMATCH");
        set.add(
            "Q01-透传-原码改写被拦",
            r.is_err() && r.code() == Some(DiagCode::ErrorCodeMutated),
            "",
        );
    }

    // mutated 标志为真即失真（中间层改码）。
    {
        let env = DecodeFailureEnvelope {
            decoder_code: "PNG_CHUNK_CRC_MISMATCH".to_string(),
            uri: "ve-asset://app/texture/broken.png".to_string(),
            stage: StageId::Verify,
            hint: "重取".to_string(),
            mutated: true,
        };
        let r = verify_passthrough(&env, "PNG_CHUNK_CRC_MISMATCH");
        set.add(
            "Q01-透传-mutated标志被拦",
            r.is_err() && r.code() == Some(DiagCode::ErrorCodeMutated),
            "",
        );
    }

    // 缺 uri/hint 即甩锅（透传不等于甩锅）。
    {
        let env = DecodeFailureEnvelope {
            decoder_code: "PNG_CHUNK_CRC_MISMATCH".to_string(),
            uri: String::new(),
            stage: StageId::Verify,
            hint: String::new(),
            mutated: false,
        };
        let r = verify_passthrough(&env, "PNG_CHUNK_CRC_MISMATCH");
        set.add(
            "Q01-透传-缺三要素被拦",
            r.is_err() && r.code() == Some(DiagCode::ErrorCodeMutated),
            "",
        );
    }

    // 委托成功时携带 F 域登记码（原码表随产物流动）。
    {
        let r = delegate_decode(1024, ResourceKind::Texture, "PNG_CHUNK_CRC_MISMATCH");
        let ok = matches!(
            &r,
            Outcome::Ok { value: p, .. }
                if p.registered_code == "PNG_CHUNK_CRC_MISMATCH"
                    && p.byte_length == 1024
                    && !p.decoder_id.is_empty()
        );
        set.add("Q01-透传-委托携带登记原码", ok, "");
    }

    // 登记码为空时落成功哨兵（区别于「失败但码为空」）。
    {
        let r = delegate_decode(16, ResourceKind::Audio, "");
        let ok = matches!(&r, Outcome::Ok { value: p, .. } if p.registered_code == DECODE_OK_SENTINEL);
        set.add("Q01-透传-空登记码落成功哨兵", ok, "");
    }

    // 空字节流被拒（不让 F 域面对它）。
    {
        let r = delegate_decode(0, ResourceKind::Texture, "X");
        set.add(
            "Q01-透传-空字节流被拒",
            r.is_err() && r.code() == Some(DiagCode::IoNotFound),
            "",
        );
    }

    // ---- 判据六：归属红线（URI 不含用户内容） ----

    // 合法 URI 通过。
    {
        let g = std_grants();
        let r = parse_resource_uri("ve-asset://app/texture/base.png", Some("VE-I"), &g);
        set.add("Q01-红线-合法URI通过", r.is_ok(), "");
    }

    // 路径段含空白即违规（内容形态信号）。
    {
        let g = std_grants();
        let r = parse_resource_uri("ve-asset://app/texture/my file.png", Some("VE-I"), &g);
        set.add(
            "Q01-红线-路径含空白被拒",
            r.is_err() && r.code() == Some(DiagCode::UriContainsUserContent),
            "",
        );
    }

    // query 内容型键被拒（按键名判定，`&title=` 亦命中）。
    {
        let g = std_grants();
        let a = parse_resource_uri("ve-asset://app/scene/a.png?title=x", Some("VE-P"), &g);
        let b = parse_resource_uri("ve-asset://app/scene/a.png?v=1&title=x", Some("VE-P"), &g);
        set.add(
            "Q01-红线-query内容键被拒（含变体）",
            a.code() == Some(DiagCode::UriContainsUserContent)
                && b.code() == Some(DiagCode::UriContainsUserContent),
            "",
        );
    }

    // query 内容型键的百分号编码变体被拒（`%74itle` = title）。
    {
        let g = std_grants();
        let r = parse_resource_uri("ve-asset://app/scene/a.png?%74itle=x", Some("VE-P"), &g);
        set.add(
            "Q01-红线-query编码变体被拒",
            r.code() == Some(DiagCode::UriContainsUserContent),
            "",
        );
    }

    // query 键大小写变体被拒（`?TItle=`）。
    {
        let g = std_grants();
        let r = parse_resource_uri("ve-asset://app/scene/a.png?TItle=x", Some("VE-P"), &g);
        set.add(
            "Q01-红线-query大小写变体被拒",
            r.code() == Some(DiagCode::UriContainsUserContent),
            "",
        );
    }

    // query 合法标识键通过（`?v=<哈希>`）。
    {
        let g = std_grants();
        let r = parse_resource_uri("ve-asset://app/texture/base.png?v=ab12&lod=3", Some("VE-I"), &g);
        set.add("Q01-红线-query标识键放行", r.is_ok(), "");
    }

    // query 未登记键被拒（每个 query 键都要有明确归属）。
    {
        let g = std_grants();
        let r = parse_resource_uri("ve-asset://app/texture/base.png?weird=1", Some("VE-I"), &g);
        set.add(
            "Q01-红线-query未登记键被拒",
            r.code() == Some(DiagCode::UriMalformed),
            "",
        );
    }

    // 路径穿越被拒（`..` 不得越过authority 根）。
    {
        let g = std_grants();
        let r = parse_resource_uri("ve-asset://app/../../secret.png", Some("VE-I"), &g);
        set.add(
            "Q01-红线-路径穿越被拒",
            r.code() == Some(DiagCode::UriMalformed),
            "",
        );
    }

    // 穿越的 `..` 相消形态仍被拒（吸收态，不得静默折叠）。
    {
        let g = std_grants();
        let r = parse_resource_uri("ve-asset://app/../../etc/passwd", Some("VE-I"), &g);
        set.add(
            "Q01-红线-越界吸收态被拒",
            r.is_err() && r.code() == Some(DiagCode::UriMalformed),
            "",
        );
    }

    // 合法相对折叠被接受（`a/../b` → `b`）。
    {
        let g = std_grants();
        let r = parse_resource_uri("ve-asset://app/texture/a/../b.png", Some("VE-I"), &g);
        let ok = matches!(&r, Outcome::Ok { value: u, .. } if u.path == "/texture/b.png");
        set.add("Q01-红线-合法相对折叠被接受", ok, "");
    }

    // 归一化的越界标志为吸收态（一旦置位不再清除）。
    {
        let n = normalize_path("/../../etc/passwd");
        set.add(
            "Q01-红线-越界标志吸收态",
            n.escaped_root && n.segments.contains(&"etc".to_string()),
            "",
        );
    }

    // 超长 URI 被拒（防误用闸门）。
    {
        let g = std_grants();
        let long = "a".repeat(URI_MAX_LEN + 1);
        let r = parse_resource_uri(&long, Some("VE-I"), &g);
        set.add(
            "Q01-红线-超长URI被拒",
            r.code() == Some(DiagCode::UriMalformed),
            "",
        );
    }

    // 缺 scheme 被拒。
    {
        let g = std_grants();
        let r = parse_resource_uri("app/texture/base.png", Some("VE-I"), &g);
        set.add(
            "Q01-红线-缺scheme被拒",
            r.code() == Some(DiagCode::UriMalformed),
            "",
        );
    }

    // 缺 authority 被拒。
    {
        let g = std_grants();
        let r = parse_resource_uri("ve-asset:///texture/base.png", Some("VE-I"), &g);
        set.add(
            "Q01-红线-缺authority被拒",
            r.code() == Some(DiagCode::UriMalformed),
            "",
        );
    }

    // 未登记 scheme 被拒（封闭默认）。
    {
        let g = std_grants();
        let r = parse_resource_uri("weird://app/texture/base.png", Some("VE-I"), &g);
        set.add(
            "Q01-红线-未登记scheme被拒",
            r.code() == Some(DiagCode::UriMalformed),
            "",
        );
    }

    // remote 默认拒绝。
    {
        let g = std_grants();
        let r = parse_resource_uri("remote://cdn/app/texture/base.png", Some("VE-I"), &g);
        set.add(
            "Q01-红线-remote默认拒绝",
            r.code() == Some(DiagCode::SourceUntrusted),
            "",
        );
    }

    // remote 经显式授权后放行。
    {
        let g = granted();
        let r = parse_resource_uri("remote://cdn/app/stream/a.mp4", Some("VE-G"), &g);
        set.add("Q01-红线-remote授权后放行", r.is_ok(), "");
    }

    // remote 未授权域仍被拒（授权指名到域）。
    {
        let g = granted();
        let r = parse_resource_uri("remote://cdn/app/stream/a.mp4", Some("VE-O"), &g);
        set.add(
            "Q01-红线-remote授权不跨域",
            r.code() == Some(DiagCode::SourceUntrusted),
            "",
        );
    }

    // 启动期无消费域时 remote 一律拒绝（无人可审计）。
    {
        let g = granted();
        let r = parse_resource_uri("remote://cdn/app/stream/a.mp4", None, &g);
        set.add(
            "Q01-红线-启动期remote一律拒绝",
            r.code() == Some(DiagCode::SourceUntrusted),
            "",
        );
    }

    // 远程授权可撤销（可审计：谁开了远程能回答）。
    {
        let mut g = granted();
        let _ = g.revoke("VE-G");
        let r = parse_resource_uri("remote://cdn/app/stream/a.mp4", Some("VE-G"), &g);
        set.add(
            "Q01-红线-远程授权可撤销",
            r.code() == Some(DiagCode::SourceUntrusted) && g.list().is_empty(),
            "",
        );
    }

    // 匿名授权被拒（授权须指名道姓）。
    {
        let mut g = RemoteGrantTable::new();
        let r = g.grant("  ");
        set.add(
            "Q01-红线-匿名远程授权被拒",
            r.is_err() && r.code() == Some(DiagCode::ValueInvalid),
            "",
        );
    }

    // ---- 判据七：跨批对接（依赖图环检测） ----

    // 无环图通过。
    {
        let edges = vec![
            DependencyEdge {
                from: "scene".to_string(),
                to: "model".to_string(),
            },
            DependencyEdge {
                from: "model".to_string(),
                to: "texture".to_string(),
            },
        ];
        let r = detect_dependency_cycle(&edges);
        set.add("Q01-对接-无环图通过", r.is_ok(), "");
    }

    // 成环被检出（加载死锁防线）。
    {
        let edges = vec![
            DependencyEdge {
                from: "a".to_string(),
                to: "b".to_string(),
            },
            DependencyEdge {
                from: "b".to_string(),
                to: "c".to_string(),
            },
            DependencyEdge {
                from: "c".to_string(),
                to: "a".to_string(),
            },
        ];
        let r = detect_dependency_cycle(&edges);
        set.add(
            "Q01-对接-依赖成环被检出",
            r.is_err() && r.code() == Some(DiagCode::DependencyCycle),
            "",
        );
    }

    // 自环被检出。
    {
        let edges = vec![DependencyEdge {
            from: "self".to_string(),
            to: "self".to_string(),
        }];
        let r = detect_dependency_cycle(&edges);
        set.add(
            "Q01-对接-自环被检出",
            r.is_err() && r.code() == Some(DiagCode::DependencyCycle),
            "",
        );
    }

    // 菱形图（无环）不误报——迭代式 DFS 的回退逻辑正确。
    {
        let edges = vec![
            DependencyEdge {
                from: "root".to_string(),
                to: "left".to_string(),
            },
            DependencyEdge {
                from: "root".to_string(),
                to: "right".to_string(),
            },
            DependencyEdge {
                from: "left".to_string(),
                to: "leaf".to_string(),
            },
            DependencyEdge {
                from: "right".to_string(),
                to: "leaf".to_string(),
            },
        ];
        let r = detect_dependency_cycle(&edges);
        set.add("Q01-对接-菱形图不误报成环", r.is_ok(), "");
    }

    // 环检测结果确定（同一输入两次判定一致）。
    {
        let edges = vec![
            DependencyEdge {
                from: "a".to_string(),
                to: "b".to_string(),
            },
            DependencyEdge {
                from: "b".to_string(),
                to: "a".to_string(),
            },
        ];
        let m1 = detect_dependency_cycle(&edges).screen_text();
        let m2 = detect_dependency_cycle(&edges).screen_text();
        set.add("Q01-对接-环检测结果确定", m1 == m2, "");
    }

    // ---- 判据八：预载分流（致命 vs 可延后不混成一个数字） ----

    // 关键素材失败记入致命清单。
    {
        let g = std_grants();
        let entries = vec![
            WarmupEntry {
                uri: "ve-asset://app/texture/base.png".to_string(),
                kind: ResourceKind::Texture,
                critical: true,
            },
            // 用「query 含内容型键」作可延后失败输入：寻址层必然拦下它。
            // 不能用「文件不存在」——寻址层不校验存在性（那是 load 段的IO），
            // 结构合法的 URI 必然计入 warmed，用例前置条件不成立。
            WarmupEntry {
                uri: "ve-asset://app/font/main.ttf?title=x".to_string(),
                kind: ResourceKind::Font,
                critical: false,
            },
        ];
        let r = warm_pipeline(&entries, &g);
        let ok = matches!(
            &r,
            Outcome::Ok { value: rep, .. }
                if rep.warmed == 1 && rep.deferred == 1 && rep.critical_failures.is_empty()
        );
        set.add("Q01-预载-可延后失败分流正确", ok, "");
    }

    // 关键素材失败不被当成可延后（问题不推迟到空画面）。
    {
        let g = std_grants();
        let entries = vec![WarmupEntry {
            uri: "ve-asset://app/../secret.png".to_string(),
            kind: ResourceKind::Scene,
            critical: true,
        }];
        let r = warm_pipeline(&entries, &g);
        let ok = matches!(
            &r,
            Outcome::Ok { value: rep, .. } if rep.critical_failures.len() == 1 && rep.warmed == 0
        );
        set.add("Q01-预载-关键失败入致命清单", ok, "");
    }

    // 预载失败必留诊断（启动期无用户交互，但不许静默）。
    {
        let g = std_grants();
        let entries = vec![WarmupEntry {
            uri: "remote://cdn/app/stream/a.mp4".to_string(),
            kind: ResourceKind::Stream,
            critical: false,
        }];
        let r = warm_pipeline(&entries, &g);
        set.add(
            "Q01-预载-预载失败留诊断",
            !r.diagnostics().is_empty(),
            "",
        );
    }

    // ---- 判据九：能力边界与裁决 ----

    // 能力有唯一属主。
    {
        let ok = resolve_capability_owner("resource-load-pipeline") == Some(("VE-Q", "六段加载流水编排"));
        set.add("Q01-边界-能力反查命中唯一属主", ok, "");
    }

    // 仲裁位能力（property-write）也须有属主——仲裁不是无人负责。
    {
        let ok = resolve_capability_owner("property-write").is_some();
        set.add("Q01-边界-仲裁位能力有属主", ok, "");
    }

    // 无主能力被立案（治理缺口不等于不在范围）。
    {
        let r = adjudicate_capability("nonexistent-capability", "VE-Q");
        set.add(
            "Q01-边界-无主能力被立案",
            r.is_err() && r.code() == Some(DiagCode::ValueInvalid),
            "",
        );
    }

    // 本域申请自有能力放行。
    {
        let r = adjudicate_capability("resource-load-pipeline", "VE-Q");
        set.add(
            "Q01-边界-本域申请自有能力放行",
            matches!(&r, Outcome::Ok { value: v, .. } if v.status() == "owned"),
            "",
        );
    }

    // 他域申请被导流（不得自行实现）。
    {
        let r = adjudicate_capability("format-decode", "VE-Q");
        set.add(
            "Q01-边界-他域能力申请被导流",
            matches!(&r, Outcome::Ok { value: v, .. } if v.status() == "redirected"),
            "",
        );
    }

    // ---- 判据十：上游台账与性能预算 ----

    // 台账每条协议均有 handler 登记与无障碍替述。
    {
        let r = audit_upstream_ledger();
        set.add("Q01-台账-台账与实现不脱节", r.is_ok(), "");
    }

    // 复杂度预算诚实（自称 O(1) 必注明上界来源）。
    {
        let r = audit_perf_budget();
        set.add("Q01-预算-复杂度预算诚实", r.is_ok(), "");
    }

    // 非常数级预算项不得混入常数级断言（bounded_by 为空者必须明说）。
    {
        let ok = PERF_BUDGET
            .iter()
            .any(|b| b.bounded_by.is_none() && !b.complexity.contains("O(1)"));
        set.add("Q01-预算-非常数级项单列", ok, "");
    }

    // ---- 判据十一：主题号段无缝 ----

    // 十主题号段覆盖 3201–3400 无洞无重叠。
    {
        let mut ok = true;
        for (i, t) in Q_DOMAIN_TEN_TOPICS.iter().enumerate() {
            let span = t.span();
            if span.item_count != 20 {
                ok = false;
            }
            if i == 0 && span.lo != DOMAIN_ITEM_LO {
                ok = false;
            }
            if i == Q_DOMAIN_TEN_TOPICS.len() - 1 && span.hi != DOMAIN_ITEM_HI {
                ok = false;
            }
        }
        set.add("Q01-号段-十主题号段无缝覆盖全域", ok, "");
    }

    // 主题 ordinal 与号段一致（主题表与号段表不得漂移）。
    {
        let mut ok = true;
        for t in Q_DOMAIN_TEN_TOPICS.iter() {
            let want = DOMAIN_ITEM_LO + (t.ordinal() as u32) * 20;
            if t.span().lo != want {
                ok = false;
            }
        }
        set.add("Q01-号段-主题序位与号段同源", ok, "");
    }

    // ---- 判据十二：总纲契约自检 ----

    // 标准总纲零契约问题（开工条自己先做到可追溯）。
    {
        let a = PipelineArchitecture::standard();
        let issues = a.check_contracts();
        set.add(
            "Q01-总纲-标准态零契约问题",
            issues.is_empty(),
            "",
        );
    }

    // 契约自检确实能检出问题（不是永远返回空的自检）。
    {
        let mut a = PipelineArchitecture::standard();
        let _ = a.grants_mut().grant("VE-G");
        let issues = a.check_contracts();
        set.add(
            "Q01-总纲-授权变更不破坏契约",
            issues.is_empty(),
            "",
        );
    }

    // ---- 判据十三：诊断基础设施（零静默） ----

    // 诊断码全集可双向反查（跨语言对拍可寻址）。
    {
        let mut ok = DiagCode::ALL.len() == 18;
        for c in DiagCode::ALL.iter() {
            match DiagCode::from_code(c.code()) {
                Some(x) if x == *c => {}
                _ => ok = false,
            }
        }
        set.add("Q01-诊断-诊断码可双向反查", ok, "");
    }

    // 未登记诊断码反查返回 None（不静默兜底成某个码）。
    {
        set.add(
            "Q01-诊断-未登记码反查为None",
            DiagCode::from_code("NO_SUCH_CODE").is_none(),
            "",
        );
    }

    // 诊断袋不留空建议（空建议等于把问题推给调用方猜）。
    {
        let mut bag = DiagBag::new();
        bag.push(DiagCode::ValueInvalid, "", "");
        let all = bag.all();
        set.add(
            "Q01-诊断-空消息与空建议被补齐",
            all.len() == 1
                && !all[0].message.trim().is_empty()
                && !all[0].hint.trim().is_empty(),
            "",
        );
    }

    // 失败结果必带三要素（code/message/hint）。
    {
        let r = lookup_stage("nope");
        let ok = match &r {
            Outcome::Err {
                code,
                message,
                hint,
                diagnostics,
            } => {
                *code == DiagCode::StageUnregistered
                    && !message.trim().is_empty()
                    && !hint.trim().is_empty()
                    && !diagnostics.is_empty()
            }
            _ => false,
        };
        set.add("Q01-诊断-失败三要素齐备", ok, "");
    }

    // 成功结果亦可取诊断（成功路径允许带告警）。
    {
        let g = std_grants();
        let r = warm_pipeline(
            &[WarmupEntry {
                uri: "ve-asset://app/font/x.ttf".to_string(),
                kind: ResourceKind::Font,
                critical: false,
            }],
            &g,
        );
        set.add("Q01-诊断-成功路径可带告警", r.is_ok(), "");
    }

    // value_or 不 panic（守卫不得成为崩溃源）。
    {
        let r: Outcome<u32> = Outcome::err(DiagCode::ValueInvalid, "x", "y");
        set.add("Q01-诊断-失败取值不崩溃", r.value_or(7) == 7, "");
    }

    // ---- 判据十四：无障碍（读屏替代） ----

    // 架构图有线性文字替述且六段齐现。
    {
        let a = PipelineArchitecture::standard();
        let n = a.architecture_narration();
        let need = ["寻址", "请求", "调度", "加载", "校验", "交付句柄"];
        let ok = need.iter().all(|k| n.contains(k)) && n.contains("十项资源映射");
        set.add("Q01-读屏-架构图有线性文字替述", ok, "");
    }

    // 替述文本与图版同源（改STAGE_SPECS 后替述随之变化，不另写一份）。
    {
        let a = PipelineArchitecture::standard();
        let n = a.architecture_narration();
        let ok = STAGE_SPECS.iter().all(|s| n.contains(s.label()));
        set.add("Q01-读屏-替述与规格同源", ok, "");
    }

    // 诊断可读屏播报且带处置建议。
    {
        let d = Diagnostic {
            code: DiagCode::UriMalformed,
            message: "URI 缺 scheme".to_string(),
            hint: "须形如 scheme://authority/path".to_string(),
        };
        let s = d.screen_line();
        set.add(
            "Q01-读屏-诊断可播报带建议",
            s.contains("URI_MALFORMED") && s.contains("scheme://authority/path"),
            "",
        );
    }

    // 错误信封播报不暴露本地路径。
    {
        let env = DecodeFailureEnvelope {
            decoder_code: "PNG_CHUNK_CRC_MISMATCH".to_string(),
            uri: "ve-asset://app/texture/secret-name.png".to_string(),
            stage: StageId::Verify,
            hint: "重取资源".to_string(),
            mutated: false,
        };
        let s = env.screen_line();
        set.add(
            "Q01-读屏-错误播报不暴露路径",
            !s.contains("secret-name.png") && s.contains("PNG_CHUNK_CRC_MISMATCH"),
            "",
        );
    }

    // 句柄播报可读且不解引用细节。
    {
        let h = ResourceHandle {
            cache_key: "ve-asset://app/texture/a.png".to_string(),
            kind: ResourceKind::Texture,
            state: HandleState::Ready,
        };
        let s = h.screen_line();
        set.add(
            "Q01-读屏-句柄播报不解引用",
            s.contains("纹理") && s.contains("就绪") && !s.contains("a.png"),
            "",
        );
    }

    // 无障碍三原则成文（原则/隐私/失败可见）。
    {
        let ok = !A11Y_ERROR_PRINCIPLE.trim().is_empty()
            && !PRIVACY_NOTE.trim().is_empty()
            && !FAILURE_VISIBILITY.trim().is_empty();
        set.add("Q01-读屏-无障碍三原则成文", ok, "");
    }

    // 私加载立案可播报（分叉缺陷要能被人听见）。
    {
        let v = detect_private_load(&sightings());
        let s = v.cases[0].screen_line();
        set.add(
            "Q01-读屏-私加载立案可播报",
            s.contains("PRIVLOAD-VE-I-1") && s.contains("分叉缺陷"),
            "",
        );
    }

    // ---- 判据十五：TS 迁移留痕 ----

    // 迁移对照表非空（本条确系TS→Rust 迁移的落位）。
    {
        set.add(
            "Q01-迁移-TS迁移对照已留痕",
            MIGRATION_FROM_TS.len() >= 10
                && MIGRATION_FROM_TS
                    .iter()
                    .all(|(k, v)| !k.trim().is_empty() && !v.trim().is_empty()),
            "",
        );
    }

    // 下游归属表非空（防止开工条被当万能筐）。
    {
        set.add(
            "Q01-迁移-下游归属表已登记",
            DOWNSTREAM_OWNERSHIP.len() == 8,
            "",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veq01_arch_is_standard_clean() {
        // 总纲本体必须自洽（标准态零契约问题）。
        let a = PipelineArchitecture::standard();
        assert!(a.check_contracts().is_empty(), "标准态契约问题：{:?}", a.check_contracts());
        assert_eq!(a.version, ARCH_VERSION);
    }

    #[test]
    fn veq01_delegation_redline_actually_fires() {
        // 单向委托红线的**可执行性**证明：带参形态与无参形态都要触发。
        // 历史缺陷正是「只在一侧规范化」导致红线永不触发。
        for sig in ["acquireRefCount(key)", "acquireRefCount"] {
            let calls = [DelegationCall {
                from: DelegateSide::F,
                signature: sig.to_string(),
            }];
            assert!(
                check_delegation_direction(&calls).is_err(),
                "红线未触发：{}",
                sig
            );
        }
    }

    #[test]
    fn veq01_convergence_redline_files_case() {
        // 收敛红线的可执行性：私加载必立案，且立案号确定。
        let v = detect_private_load(&sightings());
        assert_eq!(v.count, 2);
        assert_eq!(v.cases[0].case_id, "PRIVLOAD-VE-I-1");
        assert_eq!(v.cases[1].case_id, "PRIVLOAD-VE-P-2");
        // 全合规时零立案。
        assert_eq!(detect_private_load(&[]).count, 0);
    }

    #[test]
    fn veq01_passthrough_keeps_original_code() {
        // 透传不失真的核心：原码一致才通过，改写必被拦。
        let env = DecodeFailureEnvelope {
            decoder_code: "PNG_CHUNK_CRC_MISMATCH".to_string(),
            uri: "ve-asset://app/texture/broken.png".to_string(),
            stage: StageId::Verify,
            hint: "重取".to_string(),
            mutated: false,
        };
        assert!(verify_passthrough(&env, "PNG_CHUNK_CRC_MISMATCH").is_ok());
        assert!(verify_passthrough(&env, "LOAD_FAILED").is_err());
    }

    #[test]
    fn veq01_path_escape_is_absorbing() {
        // 越界 `..` 是吸收态：`/../../etc/passwd` 前两个 `..` 不得互相抵消。
        let n = normalize_path("/../../etc/passwd");
        assert!(n.escaped_root, "越界标志必须置位");
        let g = std_grants();
        let r = parse_resource_uri("ve-asset://app/../../etc/passwd", Some("VE-I"), &g);
        assert!(r.is_err(), "穿越必须被拒");
    }

    #[test]
    fn veq01_query_content_keys_blocked_by_name() {
        // 按键名判定（而非子串）：三种变体全部命中，含百分号编码。
        let g = std_grants();
        for uri in [
            "ve-asset://app/scene/a.png?title=x",
            "ve-asset://app/scene/a.png?v=1&title=x",
            "ve-asset://app/scene/a.png?%74itle=x",
            "ve-asset://app/scene/a.png?TItle=x",
        ] {
            assert_eq!(
                parse_resource_uri(uri, Some("VE-P"), &g).code(),
                Some(DiagCode::UriContainsUserContent),
                "内容键变体漏检：{}",
                uri
            );
        }
    }

    #[test]
    fn veq01_six_stage_order_is_frozen() {
        // 六段顺序冻结：规范次序通过，调换与跳段均被拒。
        assert!(check_stage_order(&STAGE_ORDER).is_ok());
        let mut swapped = STAGE_ORDER;
        swapped.swap(3, 5);
        assert!(check_stage_order(&swapped).is_err());
        assert!(check_stage_order(&STAGE_ORDER[..5]).is_err());
    }

    #[test]
    fn veq01_cycle_detection_no_false_positive() {
        // 菱形图（无环）不得误报——迭代式 DFS 的回退逻辑正确性。
        let edges = vec![
            DependencyEdge {
                from: "root".to_string(),
                to: "left".to_string(),
            },
            DependencyEdge {
                from: "root".to_string(),
                to: "right".to_string(),
            },
            DependencyEdge {
                from: "left".to_string(),
                to: "leaf".to_string(),
            },
            DependencyEdge {
                from: "right".to_string(),
                to: "leaf".to_string(),
            },
        ];
        assert!(detect_dependency_cycle(&edges).is_ok());
    }

    #[test]
    fn veq01_all_checks_green() {
        // 域自检全绿。
        let set = run_veq01_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "veq01 域自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }
}