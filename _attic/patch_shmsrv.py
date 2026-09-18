# 任务32 shmsrv.rs 自检修补（含回读断言）
import io

p = r"kernel/varix/src/shmsrv.rs"
s = io.open(p, encoding="utf-8").read()

# ① 删除未使用的 resolve（共享版）——死代码即 kcheck 告警源。
start = "    /// 代数核对 + 存活校验。任何错误的句柄都汇入 NotFound，绝不区分泄露细节。\n    fn resolve(&self, h: u64)"
i0 = s.index(start)
end_marker = "    fn resolve_mut(&mut self, h: u64)"
i1 = s.index(end_marker)
s = s[:i0] + s[i1:]
assert "fn resolve(&self" not in s and "fn resolve_mut" in s

# ② 探针 step⑥：destroy 后块不存在 → NotFound（resolve 失败先于槽位检查）。
old = '            && s.access(&stolen, OWNER, false) == Err(ShmError::StaleMapping)\n            && s.map(h, OWNER, false) == Err(ShmError::NotFound)'
new = '            && s.access(&stolen, OWNER, false) == Err(ShmError::NotFound)\n            && s.map(h, OWNER, false) == Err(ShmError::NotFound)'
assert s.count(old) == 1
s = s.replace(old, new, 1)

# ③ 探针尾部去掉多余行。
old = "        let _ = ok1 && ok2 && ok3 && ok4 && ok5 && ok6;\n"
assert s.count(old) == 1
s = s.replace(old, "", 1)

# ④ 测试 handle_passing：写方向走槽位检查（令牌可写）→ StaleMapping。
old = """        // 写方向同样被拒。
        assert_eq!(s.access(&m8, 9, true), Err(ShmError::PermissionDenied));"""
new = """        // 写方向同样被拒（令牌可写性成立，但 pid 复核先死于槽位）。
        assert_eq!(s.access(&m8, 9, true), Err(ShmError::StaleMapping));"""
assert s.count(old) == 1
s = s.replace(old, new, 1)

# ⑤ 测试 destroy：access 于 destroy 后 → NotFound。
old = """        s.destroy(h, 7).unwrap();
        assert_eq!(s.access(&m8, 8, false), Err(ShmError::StaleMapping));"""
new = """        s.destroy(h, 7).unwrap();
        assert_eq!(s.access(&m8, 8, false), Err(ShmError::NotFound), "块已不存在");"""
assert s.count(old) == 1
s = s.replace(old, new, 1)

# ⑥ 测试槽位填充数学：授权只给 100..104，映射填满 8 槽 = owner×4 + 授权者×4。
old = """        for pid in 100..108u32 {
            assert!(s.map(h, pid, false).is_ok(), "owner+4 授权者都可映射");
        }
        assert_eq!(s.map(h, 7, false), Err(ShmError::NoMapSlot), "第 9 个映射明确拒绝");
        // unmap 释放槽位后可复用。
        let first = Mapping { handle: h, seq: 1, writable: false };
        s.unmap(&first, 100).unwrap();
        assert!(s.map(h, 7, false).is_ok());
        assert_eq!(s.unmap(&first, 100), Err(ShmError::StaleMapping));"""
new = """        for _ in 0..4usize {
            assert!(s.map(h, 7, false).is_ok(), "owner 可重复映射（占满槽位）");
        }
        for pid in 100..104u32 {
            assert!(s.map(h, pid, false).is_ok(), "授权者可映射");
        }
        assert_eq!(s.map(h, 7, false), Err(ShmError::NoMapSlot), "第 9 个映射明确拒绝");
        // unmap 释放槽位后可复用（seq 1 = owner 首次映射）。
        let first = Mapping { handle: h, seq: 1, writable: false };
        s.unmap(&first, 7).unwrap();
        assert!(s.map(h, 7, false).is_ok());
        assert_eq!(s.unmap(&first, 7), Err(ShmError::StaleMapping));"""
assert s.count(old) == 1
s = s.replace(old, new, 1)

io.open(p, "w", encoding="utf-8", newline="").write(s)

# 回读断言
s2 = io.open(p, encoding="utf-8").read()
assert "fn resolve(&self" not in s2
assert "Err(ShmError::NotFound), \"块已不存在\"" in s2
assert "s.unmap(&first, 7)" in s2
assert "owner 可重复映射" in s2
assert "let _ = ok1" not in s2
assert s2.count("StaleMapping"), "StaleMapping 仍在"
print("shmsrv patch ok")
