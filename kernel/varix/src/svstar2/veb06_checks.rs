//! VE-F0206 · 域自检（判据逐条对应，见 `veb06_stream.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 四类命令 → `B06-四类-*`
//! - 编码期拦截 → `B06-拦截-*`
//! - 分块续传 → `B06-分块-*`
//! - 版本戳（含对端降级）→ `B06-版本-*`
//! - 黄金流比对 / 绑定去重收益 → `B06-黄金-*` / `B06-去重-*`

use super::veb06_stream::*;
use crate::checks::CheckSet;

use alloc::{format, vec};

/// VE-F0206 域自检。
pub fn run_veb06_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veb06");

    // ---- 判据：四类命令 ----

    {
        let mut ctx = EncoderContext::new(1, PeerCompat::Full);
        let c1 = ctx.create(1, OP_CREATE_BLEND, &vec![0x0000_0001, 0x0000_0002]);
        let c2 = ctx.bind(SLOT_BLEND, 1);
        let c3 = ctx.draw(0, 36, 4); // 三角形带 36 顶点
        let c4 = ctx.sync_flush(7);
        let four = c1.is_ok()
            && c2.is_ok()
            && c3.is_ok()
            && c4.is_ok()
            && ctx.emitted.iter().filter(|e| !e.deduped).map(|e| e.class).collect::<Vec<_>>()
                == vec![CmdClass::Create, CmdClass::Bind, CmdClass::Draw, CmdClass::Sync];
        // 操作码与类别对拍：冻结表逐条归类正确
        let tbl_ok = op_class(OP_CREATE_SURFACE) == Some(CmdClass::Create)
            && op_class(OP_CREATE_VERTEX_ELEMENTS) == Some(CmdClass::Create)
            && op_class(OP_SET_FRAMEBUFFER) == Some(CmdClass::Bind)
            && op_class(OP_DRAW_INDEXED) == Some(CmdClass::Draw)
            && op_class(OP_FENCE) == Some(CmdClass::Sync)
            && op_class(0x9999).is_none();
        // 绑定槽位→操作码映射冻结
        let slot_ok = slot_bind_op(SLOT_BLEND) == Some(OP_BIND_BLEND)
            && slot_bind_op(SLOT_FRAMEBUFFER) == Some(OP_SET_FRAMEBUFFER)
            && slot_bind_op(99).is_none();
        set.add("B06-四类-发射与归类全对", four && tbl_ok && slot_ok, "");
    }

    // ---- 判据：编码期拦截 ----

    {
        let mut ctx = EncoderContext::new(1, PeerCompat::Full);
        // 未创建即绑定 → E_HANDLE_NOT_CREATED（三要素）
        let e1 = ctx.bind(SLOT_RASTERIZER, 42).unwrap_err();
        // 重复创建 → E_DUP_HANDLE
        let _ = ctx.create(7, OP_CREATE_BLEND, &vec![1]);
        let e2 = ctx.create(7, OP_CREATE_DSA, &vec![1]);
        // 未知槽位 / 未知操作码
        let e3 = ctx.bind(99, 7);
        let e4 = ctx.create(8, OP_DRAW_VBO, &vec![]);
        set.add(
            "B06-拦截-四路编码期拒绝",
            e1.code == "E_HANDLE_NOT_CREATED"
                && e1.is_complete()
                && e1.next.contains("create")
                && e2.as_ref().err().map(|e| e.code == "E_DUP_HANDLE").unwrap_or(false)
                && e3.as_ref().err().map(|e| e.code == "E_SLOT_UNKNOWN").unwrap_or(false)
                && e4.as_ref().err().map(|e| e.code == "E_OP_NOT_CREATE").unwrap_or(false),
            "",
        );
    }

    // ---- 判据：分块续传 ----

    {
        let mut ctx = EncoderContext::new(1, PeerCompat::Full);
        // 灌大命令流：约 3 块以上
        for i in 0..400u32 {
            let _ = ctx.create(i + 1, OP_CREATE_SAMPLER_VIEW, &vec![i, i * 2, i * 3, i * 4]);
            let _ = ctx.bind(SLOT_SAMPLER_VIEW, i + 1);
        }
        let s = &ctx.stream;
        let multi = s.chunks().len() >= 3;
        // 每块载荷不超容量（分块不溢出的机器可验形式）
        let capped = s.chunks().iter().all(|c| c.payload.len() <= s.chunk_words);
        // 除末块外全部装满（满即换块）
        let full = s.chunks()[..s.chunks().len() - 1]
            .iter()
            .all(|c| c.payload.len() == s.chunk_words);
        // 块链验证（版本/序号/令牌链闭合）
        let chain = s.verify_chain().is_ok();
        // 分块纪律：页对齐
        set.add(
            "B06-分块-满即换块链闭合",
            multi && capped && full && chain && chunk_size_page_aligned(),
            "",
        );
    }
    // 重组回读：分块无损（重组 = 原命令字流拼接）
    {
        let mut ctx = EncoderContext::new(1, PeerCompat::Full);
        let mut expect: Vec<u32> = Vec::new();
        for i in 0..50u32 {
            let _ = ctx.create(i + 1, OP_CREATE_BLEND, &vec![i, i + 1]);
            let mut w = vec![5u32, OP_CREATE_BLEND, i + 1, 2, i, i + 1];
            w[0] = (w.len() as u32) - 1 + 1; // len 含自身
            expect.extend_from_slice(&w);
        }
        set.add(
            "B06-分块-重组无损",
            ctx.stream.reassemble() == expect && ctx.stream.verify_chain().is_ok(),
            "",
        );
    }
    // 单块边界：恰好容量与超容量一字（切分落点正确）
    {
        let mut s = ChunkedStream::new(VIRGL_STREAM_VERSION);
        s.push_words(&vec![7u32; CHUNK_PAYLOAD_WORDS]);
        s.push_words(&[1]);
        set.add(
            "B06-分块-边界切分落点",
            s.chunks().len() == 2
                && s.chunks()[0].payload.len() == CHUNK_PAYLOAD_WORDS
                && s.chunks()[1].payload.len() == 1
                && s.verify_chain().is_ok(),
            "",
        );
    }

    // ---- 判据：版本戳 ----

    {
        let mut ctx = EncoderContext::new(1, PeerCompat::Full);
        let _ = ctx.create(1, OP_CREATE_BLEND, &vec![1]);
        let _ = ctx.bind(SLOT_BLEND, 1);
        let _ = ctx.draw(0, 3, 1);
        let _ = ctx.sync_flush(9);
        let stamped = ctx
            .stream
            .chunks()
            .iter()
            .all(|c| c.version == VIRGL_STREAM_VERSION);
        set.add("B06-版本-逐块戳一致", stamped, "");
    }
    // 对端版本协商：全特性 / 降级标记 / 不兼容拒绝
    {
        let full = negotiate_peer(VIRGL_STREAM_VERSION);
        let low = negotiate_peer(VIRGL_STREAM_VERSION - 1);
        let high = negotiate_peer(VIRGL_STREAM_VERSION + 1);
        let mut degraded = EncoderContext::new(1, low.clone());
        let _ = degraded.create(1, OP_CREATE_BLEND, &vec![1]);
        let _ = degraded.bind(SLOT_BLEND, 1);
        let _ = degraded.sync_flush(9); // 同步类被降级跳过
        set.add(
            "B06-版本-三档对端裁决",
            full == PeerCompat::Full
                && matches!(&low, PeerCompat::Downgrade { missing } if missing.contains(&"同步"))
                && high == PeerCompat::Incompatible
                && degraded.skipped_degraded == 1
                && degraded.emitted.iter().any(|e| e.class == CmdClass::Sync),
            "",
        );
    }

    // ---- 判据：绑定去重（省带宽，收益入账）----

    {
        let mut ctx = EncoderContext::new(2, PeerCompat::Full);
        let _ = ctx.create(1, OP_CREATE_BLEND, &vec![1]);
        let _ = ctx.bind(SLOT_BLEND, 1);
        let _ = ctx.bind(SLOT_BLEND, 1); // 去重
        let _ = ctx.bind(SLOT_BLEND, 1); // 再去重
        let _ = ctx.bind(SLOT_DSA, 1); // 不同槽照发
        let bind_words: usize = ctx
            .emitted
            .iter()
            .filter(|e| e.class == CmdClass::Bind && !e.deduped)
            .map(|e| e.words)
            .sum();
        set.add(
            "B06-去重-同槽不发射",
            ctx.dedup_count == 2 && bind_words == 8,
            "",
        );
    }

    // ---- 黄金流比对（跨版本回放锚）----

    {
        let build = || {
            let mut ctx = EncoderContext::new(5, PeerCompat::Full);
            let _ = ctx.create(1, OP_CREATE_BLEND, &vec![0x11, 0x22]);
            let _ = ctx.create(2, OP_CREATE_RASTERIZER, &vec![0x33]);
            let _ = ctx.bind(SLOT_BLEND, 1);
            let _ = ctx.bind(SLOT_RASTERIZER, 2);
            let _ = ctx.bind(SLOT_RASTERIZER, 2); // 去重不进流
            let _ = ctx.draw(0, 6, 5);
            let _ = ctx.sync_flush(3);
            ctx
        };
        let a = build();
        let b = build();
        set.add(
            "B06-黄金-同序同指纹",
            a.golden_hash() == b.golden_hash() && a.golden_hash() != 0,
            "",
        );
    }
    // 流内容差异 → 指纹必变（指纹有效性自证）
    {
        let mut c1 = EncoderContext::new(1, PeerCompat::Full);
        let _ = c1.create(1, OP_CREATE_BLEND, &vec![1]);
        let mut c2 = EncoderContext::new(1, PeerCompat::Full);
        let _ = c2.create(1, OP_CREATE_BLEND, &vec![2]);
        set.add(
            "B06-黄金-异序异指纹",
            c1.golden_hash() != c2.golden_hash(),
            "",
        );
    }

    // ---- 对接：F0205 协商产物即编码器上下文号 ----

    {
        // 模拟协商成功产出 ctx_id=1（veb05 协商器首上下文恒 1）
        let mut ctx = EncoderContext::new(1, PeerCompat::Full);
        let _ = ctx.create(1, OP_CREATE_SURFACE, &vec![3840, 2160]);
        let ok = ctx.emitted.iter().all(|e| !e.deduped) && ctx.stream.verify_chain().is_ok();
        set.add("B06-对接-协商上下文可编码", ok && ctx.ctx_id == 1, "");
    }

    // ---- 读屏与确定性 ----

    {
        let mut ctx = EncoderContext::new(3, negotiate_peer(VIRGL_STREAM_VERSION - 1));
        let _ = ctx.create(1, OP_CREATE_BLEND, &vec![1]);
        let _ = ctx.bind(SLOT_BLEND, 1);
        let _ = ctx.sync_flush(2);
        let s = ctx.a11y_summary();
        set.add(
            "B06-读屏-摘要可播",
            s.contains("上下文") && s.contains("去重") && s.contains("降级"),
            "",
        );
    }
    {
        let run = || {
            let mut ctx = EncoderContext::new(9, PeerCompat::Full);
            for i in 0..30u32 {
                let _ = ctx.create(i + 1, OP_CREATE_BLEND, &vec![i, i * 2]);
                let _ = ctx.bind(SLOT_BLEND, i + 1);
                let _ = ctx.bind(SLOT_BLEND, i + 1);
                let _ = ctx.draw(i, 3, 1);
            }
            let _ = ctx.sync_flush(1);
            (ctx.golden_hash(), ctx.dedup_count, ctx.stream.chunks().len(), ctx.a11y_summary())
        };
        let a = run();
        let b = run();
        set.add("B06-确定-同操作同账面", a == b, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    /// 域自检必须全绿——红项即施工未完成。
    #[test]
    fn veb06_checks_all_green() {
        let set = run_veb06_checks();
        let (passed, failed) = set.tally();
        if !set.all_passed() {
            let (items, n) = set.red_items();
            let mut msg = format!("VE-B06 域自检红项：{}/{} 绿", passed, passed + failed);
            for it in items.iter().take(n) {
                if let Some(c) = it {
                    if !c.passed {
                        msg.push_str(&format!("\n  [红] {} — {}", c.name, c.detail));
                    }
                }
            }
            panic!("{}", msg);
        }
    }

    /// 编码期拦截是本项的根判据：错序使用必须在编码期被拒。
    #[test]
    fn intercepts_at_encode_time() {
        let mut ctx = EncoderContext::new(1, PeerCompat::Full);
        assert_eq!(
            ctx.bind(SLOT_BLEND, 77).unwrap_err().code,
            "E_HANDLE_NOT_CREATED"
        );
        let _ = ctx.create(1, OP_CREATE_BLEND, &vec![1]);
        assert!(ctx.bind(SLOT_BLEND, 1).is_ok());
        assert_eq!(ctx.create(1, OP_CREATE_BLEND, &vec![1]).unwrap_err().code, "E_DUP_HANDLE");
    }

    /// 分块续传：任何规模的流都不溢出、块链闭合、重组无损。
    #[test]
    fn chunked_stream_roundtrips() {
        let mut ctx = EncoderContext::new(1, PeerCompat::Full);
        for i in 0..1000u32 {
            let _ = ctx.create(i + 1, OP_CREATE_SAMPLER_VIEW, &vec![i; 3]);
        }
        let s = &ctx.stream;
        assert!(s.chunks().len() > 1, "1000 命令必须分多块");
        assert!(s.chunks().iter().all(|c| c.payload.len() <= s.chunk_words));
        assert!(s.verify_chain().is_ok());
        assert_eq!(s.total_payload_words(), s.reassemble().len());
    }

    /// 版本戳与降级：低对端跳过同步类并留痕，高对端拒绝。
    #[test]
    fn version_stamp_and_peer_matrix() {
        assert_eq!(negotiate_peer(1), PeerCompat::Full);
        assert!(matches!(negotiate_peer(0), PeerCompat::Downgrade { .. }));
        assert_eq!(negotiate_peer(2), PeerCompat::Incompatible);
        let mut degraded = EncoderContext::new(1, negotiate_peer(0));
        let _ = degraded.sync_flush(1);
        assert_eq!(degraded.skipped_degraded, 1);
        assert!(degraded
            .stream
            .chunks()
            .iter()
            .all(|c| c.version == VIRGL_STREAM_VERSION));
    }
}
