# -*- coding: utf-8 -*-
"""任务37 测试追加：冲突仲裁 10 组 + 热更新 + Arbiter 可插"""
import io

P = 'kernel/varix/src/vfsguard.rs'
s = io.open(P, encoding='utf-8').read()

anchor = """    // ---------------- 审计账本 ----------------"""
tests = """    // ---------------- 任务37 · 冲突仲裁（拒绝优先）----------------

    #[test]
    fn conflict_matrix_deny_wins_10_combos() {
        // 冲突组合 ≥10 组断言拒绝优先（总案验收口径）。
        // 矩阵：deny 覆盖读/写 × allow 精确/前缀/通配/叠加 × 嵌套目录。
        let mut rs = RuleSet::new();
        // 规则 0-1：宽 allow 打底。
        rs.add(true, true, b"/data").unwrap();
        rs.add(true, true, b"/pub/*").unwrap();
        // 规则 2-7：deny 层。
        rs.add_deny(true, true, b"/data/secret").unwrap();        // 2 整目录拒
        rs.add_deny(false, true, b"/data/rofile").unwrap();       // 3 仅拒写
        rs.add_deny(true, false, b"/data/nord").unwrap();         // 4 仅拒读
        rs.add_deny(true, true, b"/pub/inner/*").unwrap();        // 5 通配拒
        rs.add_deny(true, true, b"/data").unwrap();               // 6 父目录拒（宽压窄）
        rs.add_deny(false, true, b"/data/w").unwrap();            // 7 单操作拒
        let allow = |d: Decision| d.allow;
        // ① 精确 allow 前缀 vs deny 子目录（读）→ deny 优先。
        assert!(!allow(rs.adjudicate(b"/data/secret/k.txt", Op::Read)));
        assert_eq!(rs.adjudicate(b"/data/secret/k.txt", Op::Read).deny, Some(DenyReason::Denied));
        // ② 同上（写）。
        assert!(!allow(rs.adjudicate(b"/data/secret/k.txt", Op::Write)));
        // ③ allow 宽目录 /data vs deny 文件 rofile：读放行（deny 只覆盖写）。
        assert!(allow(rs.adjudicate(b"/data/rofile", Op::Read)));
        // ④ 但写 → deny 优先。
        assert!(!allow(rs.adjudicate(b"/data/rofile", Op::Write)));
        assert_eq!(rs.adjudicate(b"/data/rofile", Op::Write).deny, Some(DenyReason::Denied));
        // ⑤ deny 只拒读：读拒、写放行。
        assert!(!allow(rs.adjudicate(b"/data/nord", Op::Read)));
        assert!(allow(rs.adjudicate(b"/data/nord", Op::Write)));
        // ⑥ 通配 allow /pub/* vs 通配 deny /pub/inner/*。
        assert!(allow(rs.adjudicate(b"/pub/a.txt", Op::Write)));
        assert!(!allow(rs.adjudicate(b"/pub/inner/x.txt", Op::Write)));
        // ⑦ 宽 deny 父目录压过窄 allow 子目录（拒绝优先与规则顺序无关）。
        assert!(!allow(rs.adjudicate(b"/data/anything", Op::Read)));
        // ⑧ 单操作 deny：写拒读放。
        assert!(!allow(rs.adjudicate(b"/data/w/f", Op::Write)));
        assert!(allow(rs.adjudicate(b"/data/w/f", Op::Read)));
        // ⑨ deny 后仍无 allow 覆盖 → 也是拒（deny 优先，原因同为 Denied）。
        let mut rs2 = RuleSet::new();
        rs2.add_deny(true, true, b"/onlydeny").unwrap();
        assert_eq!(rs2.adjudicate(b"/onlydeny", Op::Read).deny, Some(DenyReason::Denied));
        // ⑩ 逃逸路径即使 deny 也在场 → BadPath 优先于一切规则。
        rs.add_deny(true, true, b"/data").unwrap();
        assert_eq!(
            rs.adjudicate(b"/data/../secret", Op::Read).deny,
            Some(DenyReason::BadPath(GuardError::Escape))
        );
    }

    #[test]
    fn parse_deny_lines_and_bad_lines() {
        let mut rs = RuleSet::new();
        let (ok, bad) = rs.parse(b"allow rw /a\\ndeny r /a/priv\\nbogus\\ndeny x /bad\\n");
        assert_eq!(ok, 2);
        assert_eq!(bad, 2);
        assert!(rs.adjudicate(b"/a/pub.txt", Op::Read).allow);
        assert!(!rs.adjudicate(b"/a/priv.txt", Op::Read).allow);
        assert_eq!(rs.adjudicate(b"/a/priv.txt", Op::Read).deny, Some(DenyReason::Denied));
    }

    // ---------------- 任务37 · 热更新（RuleBook）----------------

    #[test]
    fn rulebook_hot_reload_and_idempotent() {
        let mut rb = RuleBook::new();
        rb.reload(b"allow rw /handoff\\n");
        assert_eq!(rb.generation(), 1);
        assert!(rb.adjudicate(b"/handoff/f", Op::Write).allow);
        // 改文件不重启生效：新文本 → 世代 +1、新规则即刻可判。
        let rep = rb.reload(b"allow rw /handoff\\ndeny rw /handoff/secret\\n");
        assert!(rep.changed);
        assert_eq!(rep.gen_before, 1);
        assert_eq!(rep.gen_after, 2);
        assert_eq!(rb.generation(), 2);
        assert!(!rb.adjudicate(b"/handoff/secret/x", Op::Write).allow);
        // 幂等：同文本 reload → changed=false、世代不变。
        let rep2 = rb.reload(b"allow rw /handoff\\ndeny rw /handoff/secret\\n");
        assert!(!rep2.changed);
        assert_eq!(rep2.gen_after, 2);
        assert_eq!(rb.reload_count(), 2);
        // 规则删减同样生效（回滚场景）。
        rb.reload(b"allow r /pub\\n").unwrap();
        assert!(rb.adjudicate(b"/pub/r.txt", Op::Read).allow);
        assert!(!rb.adjudicate(b"/handoff/f", Op::Write).allow, "旧规则应随热更新消失");
    }

    #[test]
    fn rulebook_snapshot_race_semantics() {
        // 竞态口径：请求要么旧世代要么新世代，不存在半更新。
        // 模拟：决策引用的世代与裁决结果绑定，reload 后旧请求结果仍自洽。
        let mut rb = RuleBook::new();
        rb.reload(b"allow rw /data\\n");
        let gen_old = rb.generation();
        let d_old = rb.adjudicate(b"/data/f", Op::Write);
        assert!(d_old.allow);
        // 更新中请求（更新与请求交错：先取决策再 reload）。
        rb.reload(b"deny rw /data\\n");
        assert_eq!(rb.generation(), gen_old + 1);
        // 旧世代决策不受新规则回溯影响（快照自洽）。
        assert!(d_old.allow, "旧世代快照决策不可被回溯改写");
        // 新请求落新世代。
        assert!(!rb.adjudicate(b"/data/f", Op::Write).allow);
        assert_eq!(rb.adjudicate(b"/data/f", Op::Write).deny, Some(DenyReason::Denied));
    }

    // ---------------- 任务37 · Arbiter 可插裁决器 ----------------

    /// 新资源类型裁决器样例（总案开放性验证）：注册表键命名空间，
    /// 复用 NormPath 语义（`\\Registry\\HKEY` 折叠为 `/registry/hkey` 前缀）。
    struct RegistryArbiter {
        book: RuleBook,
    }
    impl Arbiter for RegistryArbiter {
        fn kind(&self) -> &'static str {
            "registry-key"
        }
        fn adjudicate(&self, raw: &[u8], op: Op) -> Decision {
            // 资源名适配：反斜杠 → 斜杠，接入同一 Decision 通道。
            let mut p = alloc::vec::Vec::with_capacity(raw.len() + 1);
            p.push(b'/');
            for &b in raw {
                p.push(if b == b'\\\\' { b'/' } else { b.to_ascii_lowercase() });
            }
            self.book.adjudicate(&p, op)
        }
    }

    #[test]
    fn arbiter_trait_pluggable_new_resource_type() {
        // 路径裁决器（首个内建实现）。
        let mut rb = RuleBook::new();
        rb.reload(b"allow rw /handoff\\n");
        assert_eq!(rb.kind(), "vfs-path");
        assert!(Arbiter::adjudicate(&rb, b"/handoff/f", Op::Write).allow);
        // 新资源类型：同一 trait、同一 Decision 通道、互不串扰。
        let mut rbook = RuleBook::new();
        rbook.reload(b"allow rw /registry/hkey_cu/software/varix\\n");
        let reg = RegistryArbiter { book: rbook };
        assert_eq!(reg.kind(), "registry-key");
        assert!(reg.adjudicate(b"\\\\Registry\\\\HKEY_CU\\\\Software\\\\VARIX\\\\Run", Op::Write).allow);
        assert!(!reg.adjudicate(b"\\\\Registry\\\\HKEY_CU\\\\System\\\\Run", Op::Write).allow);
        // 审计通道形状一致（Decision 字段语义共用）。
        let d = reg.adjudicate(b"\\\\Registry\\\\HKEY_CU\\\\System\\\\Run", Op::Write);
        assert_eq!(d.deny, Some(DenyReason::NoRule));
    }

    // ---------------- 审计账本 ----------------"""

assert anchor in s, 'test anchor miss'
s = s.replace(anchor, tests, 1)
io.open(P, 'w', encoding='utf-8', newline='\n').write(s)
import ast
print('tests appended')
