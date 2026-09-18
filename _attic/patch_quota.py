# -*- coding: utf-8 -*-
"""quota.rs 编译修补：类型统一 + trait 对象数组作用域重构（探针 step3 + 两个水位测试）"""
import io

p = r"kernel/varix/src/quota.rs"
s = io.open(p, encoding="utf-8").read()

# ① step1 类型统一（u64 份额 vs u32 阈值）
old = "if shares[i] + 80 < floors[i] || shares[i] > targets[i] + 80 {"
new = "if shares[i] + 80 < floors[i] as u64 || shares[i] > targets[i] as u64 + 80 {"
assert s.count(old) == 1, "step1 cmp not found"
s = s.replace(old, new, 1)

# ② 探针 step3 整块重构：trait 数组进内层作用域，账本(owed)断言在内、mock 直查在外
start = "        // ③ 水位演练：Pressure 只级1；Critical 级1→级2→3 顺序；回升逆序归还。"
end = "        all_ok &= step3;"
i0 = s.index(start)
i1 = s.index(end) + len(end)
KINFO_DRILL = (
    '            crate::kinfo!(\n'
    '                "quota: watermark drill normal/pressure/critical/normal \\\\\n'
    '                 reclaimed={} restored={} order=[ram,engine,wine] verdict={}",\n'
    '                reclaimed_before_restore,\n'
    '                wm.restored_pages(),\n'
    '                if ok { "ok" } else { "FAIL" }\n'
    '            );\n'
)
new_step3 = (
    "        // ③ 水位演练：Pressure 只级1；Critical 级1→级2→3 顺序；回升逆序归还。\n"
    "        let step3 = {\n"
    "            let mut ram = MockStage::new(ReclaimStageId::RamCache, 150);\n"
    "            let mut engine = MockStage::new(ReclaimStageId::EnginePages, 100);\n"
    "            let mut wine = MockStage::new(ReclaimStageId::WineCache, 100);\n"
    "            let mut wm = MemWatermark::new();\n"
    "            let mut ok = true;\n"
    "            let mut reclaimed_before_restore = 0u64;\n"
    "            {\n"
    "                let mut stages: [&mut dyn ReclaimStage; 3] = [&mut ram, &mut engine, &mut wine];\n"
    "                // Normal：不动。\n"
    "                ok &= wm.service(500, 1000, &mut stages[..]) == WatermarkLevel::Normal\n"
    "                    && wm.owed(ReclaimStageId::RamCache) == 0;\n"
    "                // Pressure（200‰）：只级1 动（need = 300−200 = 100）。\n"
    "                ok &= wm.service(200, 1000, &mut stages[..]) == WatermarkLevel::Pressure\n"
    "                    && wm.owed(ReclaimStageId::RamCache) == 100\n"
    "                    && wm.owed(ReclaimStageId::EnginePages) == 0\n"
    "                    && wm.owed(ReclaimStageId::WineCache) == 0;\n"
    "                // Critical（80‰）：need = 300−80 = 220 → 级1 剩 50 → 级2 100 → 级3 70。\n"
    "                // 份额算术唯一确定顺序：级3 拿 70（非 100）当且仅当级2 先于它拿满 100。\n"
    "                ok &= wm.service(80, 1000, &mut stages[..]) == WatermarkLevel::Critical\n"
    "                    && wm.owed(ReclaimStageId::RamCache) == 150\n"
    "                    && wm.owed(ReclaimStageId::EnginePages) == 100\n"
    "                    && wm.owed(ReclaimStageId::WineCache) == 70;\n"
    "                reclaimed_before_restore = wm.reclaimed_pages();\n"
    "                // 回升（400‰ ≥ 300）：逆序归还，欠账清零，归还量 = 回收量。\n"
    "                ok &= wm.service(400, 1000, &mut stages[..]) == WatermarkLevel::Normal\n"
    "                    && wm.owed(ReclaimStageId::RamCache) == 0\n"
    "                    && wm.owed(ReclaimStageId::EnginePages) == 0\n"
    "                    && wm.owed(ReclaimStageId::WineCache) == 0\n"
    "                    && wm.restored_pages() == reclaimed_before_restore;\n"
    "            }\n"
    "            // mock 直查（trait 对象借用已结束）：调用次数证明 Pressure 只动级1、\n"
    "            // Critical 三级各一次；given 清零 + 归还量逐级对账。\n"
    "            ok &= ram.reclaim_calls() == 2\n"
    "                && engine.reclaim_calls() == 1\n"
    "                && wine.reclaim_calls() == 1\n"
    "                && ram.given() == 0\n"
    "                && engine.given() == 0\n"
    "                && wine.given() == 0\n"
    "                && ram.restored() == 150\n"
    "                && engine.restored() == 100\n"
    "                && wine.restored() == 70;\n"
    + KINFO_DRILL +
    "\n"
    "            // 滞回带：280‰ 维持 Normal；240‰ 升 Pressure；270‰ 带内维持；\n"
    "            // 310‰ 才降 Normal。\n"
    "            let mut wm2 = MemWatermark::new();\n"
    "            let ok2 = wm2.evaluate(280, 1000) == WatermarkLevel::Normal\n"
    "                && wm2.evaluate(240, 1000) == WatermarkLevel::Pressure\n"
    "                && wm2.evaluate(270, 1000) == WatermarkLevel::Pressure\n"
    "                && wm2.evaluate(310, 1000) == WatermarkLevel::Normal;\n"
    "            crate::kinfo!(\n"
    "                \"quota: hysteresis 280-hold/240-up/270-hold/310-down verdict={}\",\n"
    "                if ok2 { \"ok\" } else { \"FAIL\" }\n"
    "            );\n"
    "            ok &= ok2;\n"
    "            ok\n"
    "        };\n"
    "        all_ok &= step3;"
)
s = s[:i0] + new_step3 + s[i1:]

# ③ 测试一：watermark_pressure_ramcache_only —— 账本断言进作用域，mock 直查在外
t_start = "    fn watermark_pressure_ramcache_only() {"
t_end = "    #[test]\n    fn watermark_critical_order_and_restore"
i0 = s.index(t_start)
i1 = s.index(t_end)
new_t1 = (
    "    fn watermark_pressure_ramcache_only() {\n"
    "        let mut ram = MockStage::new(ReclaimStageId::RamCache, 150);\n"
    "        let mut engine = MockStage::new(ReclaimStageId::EnginePages, 100);\n"
    "        let mut wine = MockStage::new(ReclaimStageId::WineCache, 100);\n"
    "        let mut wm = MemWatermark::new();\n"
    "        {\n"
    "            let mut stages: [&mut dyn ReclaimStage; 3] = [&mut ram, &mut engine, &mut wine];\n"
    "            // 正常水位：零动作。\n"
    "            assert_eq!(wm.service(500, 1000, &mut stages[..]), WatermarkLevel::Normal);\n"
    "            assert_eq!(wm.owed(ReclaimStageId::RamCache), 0);\n"
    "            // 压力水位：只级1，且量 = 目标 − free = 300 − 200 = 100。\n"
    "            assert_eq!(wm.service(200, 1000, &mut stages[..]), WatermarkLevel::Pressure);\n"
    "            assert_eq!(wm.owed(ReclaimStageId::RamCache), 100);\n"
    "            assert_eq!(wm.owed(ReclaimStageId::EnginePages), 0);\n"
    "            assert_eq!(wm.owed(ReclaimStageId::WineCache), 0);\n"
    "        }\n"
    "        // mock 直查：级2/级3 零调用。\n"
    "        assert_eq!(ram.given(), 100);\n"
    "        assert_eq!(engine.reclaim_calls(), 0);\n"
    "        assert_eq!(wine.reclaim_calls(), 0);\n"
    "    }\n"
    "\n"
    "    "
)
s = s[:i0] + new_t1 + s[i1:]

# ④ 测试二：watermark_critical_order_and_restore 同款重构
t_start = "    fn watermark_critical_order_and_restore() {"
t_end = "    #[test]\n    fn watermark_hysteresis_band"
i0 = s.index(t_start)
i1 = s.index(t_end)
new_t2 = (
    "    fn watermark_critical_order_and_restore() {\n"
    "        let mut ram = MockStage::new(ReclaimStageId::RamCache, 150);\n"
    "        let mut engine = MockStage::new(ReclaimStageId::EnginePages, 100);\n"
    "        let mut wine = MockStage::new(ReclaimStageId::WineCache, 100);\n"
    "        let mut wm = MemWatermark::new();\n"
    "        let mut reclaimed_before_restore = 0u64;\n"
    "        {\n"
    "            let mut stages: [&mut dyn ReclaimStage; 3] = [&mut ram, &mut engine, &mut wine];\n"
    "            let _ = wm.service(200, 1000, &mut stages[..]); // 先压到 Pressure，级1 已给 100\n"
    "            // 危急：220 缺口 → 50+100+70，算术唯一确定顺序 [ram, engine, wine]。\n"
    "            assert_eq!(wm.service(80, 1000, &mut stages[..]), WatermarkLevel::Critical);\n"
    "            assert_eq!(wm.owed(ReclaimStageId::RamCache), 150);\n"
    "            assert_eq!(wm.owed(ReclaimStageId::EnginePages), 100);\n"
    "            assert_eq!(wm.owed(ReclaimStageId::WineCache), 70);\n"
    "            reclaimed_before_restore = wm.reclaimed_pages();\n"
    "            // 回升归还：逆序、账目两清。\n"
    "            assert_eq!(wm.service(400, 1000, &mut stages[..]), WatermarkLevel::Normal);\n"
    "            assert_eq!(wm.owed(ReclaimStageId::RamCache), 0);\n"
    "            assert_eq!(wm.owed(ReclaimStageId::EnginePages), 0);\n"
    "            assert_eq!(wm.owed(ReclaimStageId::WineCache), 0);\n"
    "        }\n"
    "        assert_eq!(wm.restored_pages(), reclaimed_before_restore);\n"
    "        assert_eq!(ram.restored(), 150);\n"
    "        assert_eq!(engine.restored(), 100);\n"
    "        assert_eq!(wine.restored(), 70);\n"
    "        assert_eq!(ram.given(), 0);\n"
    "        assert_eq!(engine.given(), 0);\n"
    "        assert_eq!(wine.given(), 0);\n"
    "    }\n"
    "\n"
    "    "
)
s = s[:i0] + new_t2 + s[i1:]

io.open(p, "w", encoding="utf-8", newline="").write(s)

# 回读断言
s2 = io.open(p, encoding="utf-8").read()
assert "floors[i] as u64" in s2, "fix1 missing"
assert s2.count("ram.reclaim_calls() == 2") == 1, "fix2 missing"
assert s2.count("engine.reclaim_calls(), 0") == 1, "fix3 missing"
assert s2.count("reclaimed_before_restore") >= 2, "fix4 missing"
assert "fn watermark_hysteresis_band" in s2 and "fn gpu_soft_channel_reserve_and_accounting" in s2
# kinfo 反斜杠续行核对（Rust 宏里的行继续符是单个反斜杠）
assert "\\\\\n                 reclaimed=" not in s2, "double backslash leaked"
assert "\\\n                 reclaimed=" in s2, "kinfo line-continuation missing"
print("quota.rs patched ok")
