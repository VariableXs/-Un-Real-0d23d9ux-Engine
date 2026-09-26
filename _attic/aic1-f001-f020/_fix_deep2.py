# -*- coding: utf-8 -*-
"""深化批次二：六处断言/实现修复（AI-C1）。"""
import io

# 1) dblrun.rs: charge 恢复 breach 计数 + 呼吸断言按真实三角波形状
p = 'kernel/varix/src/compatstar/dblrun.rs'
d = io.open(p, encoding='utf-8').read()
old_charge = '''    pub fn charge(&mut self, cpu_permille: u32) -> bool {
        if cpu_permille > self.peak_permille {
            self.peak_permille = cpu_permille;
        }
        cpu_permille > CPU_CAP_PERMILLE
    }'''
new_charge = '''    pub fn charge(&mut self, cpu_permille: u32) -> bool {
        if cpu_permille > self.peak_permille {
            self.peak_permille = cpu_permille;
        }
        let breach = cpu_permille > CPU_CAP_PERMILLE;
        if breach {
            self.breaches += 1;
        }
        breach
    }'''
assert old_charge in d, 'charge pattern'
d = d.replace(old_charge, new_charge, 1)
old_assert = '''    cs.add(
        "breath_triangle_wave",
        a0 == 400 && aend == 400 && amid >= 695 && amid <= 705 && in_range,
        "",
    );'''
new_assert = '''    // 三角波形状：半周期到峰（1000），1/4 周期处为中值 700，两端回 400。
    let quarter = breath_alpha_at(BREATH_PERIOD_MS / 4);
    cs.add(
        "breath_triangle_wave",
        a0 == 400 && aend == 400 && amid == 1000 && quarter >= 695 && quarter <= 705 && in_range,
        "",
    );'''
assert old_assert in d, 'breath pattern'
d = d.replace(old_assert, new_assert, 1)
io.open(p, 'w', encoding='utf-8', newline='\n').write(d)

# 2) fsredir.rs: parity 用例对齐 USER_PASS_THROUGH（Public 四目录）
p = 'kernel/varix/src/compatstar/fsredir.rs'
d = io.open(p, encoding='utf-8').read()
d = d.replace('let d4 = classify_path("C:\\\\Users\\\\v\\\\Documents\\\\report.docx");',
              'let d4 = classify_path("C:\\\\Users\\\\Public\\\\Documents\\\\report.docx");', 1)
io.open(p, 'w', encoding='utf-8', newline='\n').write(d)

# 3) envsess.rs: 先取布尔再覆写缓冲
p = 'kernel/varix/src/compatstar/envsess.rs'
d = io.open(p, encoding='utf-8').read()
old = '''    let n1 = temp_for_app("AppA", &mut b);
    let n2 = temp_for_app("AppB", &mut b);
    cs.add(
        "temp_per_app_sandboxed",
        n1 > 0
            && n2 > 0
            && n1 == n2
            && core::str::from_utf8(&b[..n1]).unwrap().contains("AppSandbox\\\\AppA\\\\tmp")
            && core::str::from_utf8(&b[..n2]).unwrap().contains("AppSandbox\\\\AppB\\\\tmp"),
        "",
    );'''
new = '''    let n1 = temp_for_app("AppA", &mut b);
    let has_a = core::str::from_utf8(&b[..n1]).unwrap().contains("AppSandbox\\\\AppA\\\\tmp");
    let n2 = temp_for_app("AppB", &mut b);
    let has_b = core::str::from_utf8(&b[..n2]).unwrap().contains("AppSandbox\\\\AppB\\\\tmp");
    cs.add(
        "temp_per_app_sandboxed",
        n1 > 0 && n2 > 0 && n1 == n2 && has_a && has_b,
        "",
    );'''
assert old in d, 'envsess pattern'
d = d.replace(old, new, 1)
io.open(p, 'w', encoding='utf-8', newline='\n').write(d)

# 4) lnkfile.rs: 每次调用后立即比对
p = 'kernel/varix/src/compatstar/lnkfile.rs'
d = io.open(p, encoding='utf-8').read()
old = '''    let n1 = expand_target("%PROGRAMFILES%\\\\Old\\\\tool.exe", lk_env, &mut buf);
    let n2 = expand_target("%GHOST%\\\\x", lk_env, &mut buf);
    let n3 = expand_target("100%", lk_env, &mut buf);
    cs.add(
        "expand_target_single_pass",
        n1 > 0
            && &buf[..n1] == b"C:\\\\Program Files\\\\Old\\\\tool.exe"
            && n2 > 0
            && &buf[..n2] == b"%GHOST%\\\\x"
            && n3 == 4
            && &buf[..n3] == b"100%",
        "",
    );'''
new = '''    let n1 = expand_target("%PROGRAMFILES%\\\\Old\\\\tool.exe", lk_env, &mut buf);
    let hit = n1 > 0 && &buf[..n1] == b"C:\\\\Program Files\\\\Old\\\\tool.exe";
    let n2 = expand_target("%GHOST%\\\\x", lk_env, &mut buf);
    let ghost = n2 > 0 && &buf[..n2] == b"%GHOST%\\\\x";
    let n3 = expand_target("100%", lk_env, &mut buf);
    let literal = n3 == 4 && &buf[..n3] == b"100%";
    cs.add(
        "expand_target_single_pass",
        hit && ghost && literal,
        "",
    );'''
assert old in d, 'lnkfile pattern'
d = d.replace(old, new, 1)
io.open(p, 'w', encoding='utf-8', newline='\n').write(d)

# 5) persrc.rs: pick_largest 用只有 16/48 的夹具
p = 'kernel/varix/src/compatstar/persrc.rs'
d = io.open(p, encoding='utf-8').read()
old = '''    // 4) 全小于目标 → 最大档：目标 128（只有 16/48）→ 取 48。
    let p128 = pick_group_member(&g, 128).unwrap();
    cs.add("pick_largest_when_all_smaller", p128.icon_id == 2, "");'''
new = '''    // 4) 全小于目标 → 最大档：目标 128、组内只有 16/48 → 取 48；
    //    g 含 256 档 → 同目标就近放大取 256（两语义一并钉死）。
    let small = [
        GroupIconEntry { width: 16, height: 16, color_count: 0, planes: 1, bit_count: 32, bytes_in_res: 1, icon_id: 1 },
        GroupIconEntry { width: 48, height: 48, color_count: 0, planes: 1, bit_count: 32, bytes_in_res: 1, icon_id: 2 },
    ];
    let p128 = pick_group_member(&small, 128).unwrap();
    let p_up = pick_group_member(&g, 128).unwrap();
    cs.add("pick_largest_when_all_smaller", p128.icon_id == 2 && p_up.icon_id == 3, "");'''
assert old in d, 'persrc pattern'
d = d.replace(old, new, 1)
io.open(p, 'w', encoding='utf-8', newline='\n').write(d)

# 6) mlangres.rs: 既有测试更新到「已验证锚点字」新状态（空子集 → 起建）
p = 'kernel/varix/src/compatstar/mlangres.rs'
d = io.open(p, encoding='utf-8').read()
old = '''    #[test]
    fn big5_and_sjis_tables_present() {
        // Big5/Shift-JIS 表面在位（当前空子集 = 全 U+FFFD 计数路径——全量表
        // 编译入镜像时补齐，登记完成报告）。
        let r = decode(CodePage::Big5, &[0xA4, 0x40]);
        assert_eq!(r.replacement_chars, 1, "未命中表 → U+FFFD 计数（不静默猜）");
        let r2 = decode(CodePage::ShiftJis, &[0x82, 0x60]);
        assert_eq!(r2.replacement_chars, 1);
    }'''
new = '''    #[test]
    fn big5_and_sjis_tables_present() {
        // 深化批次二起建已验证锚点字子集：命中 → 正常解码；未验证对仍走
        // U+FFFD 计数（不静默猜）——全量表编译入镜像时继续补齐（登记报告）。
        let r = decode(CodePage::Big5, &[0xA4, 0x40]);
        assert_eq!(&r.utf8[..r.len], "一".as_bytes(), "锚点字命中");
        assert_eq!(r.replacement_chars, 0);
        let r2 = decode(CodePage::ShiftJis, &[0x82, 0x60]);
        assert_eq!(r2.replacement_chars, 1, "未验证对 → U+FFFD 计数");
    }'''
assert old in d, 'mlangres test pattern'
d = d.replace(old, new, 1)
io.open(p, 'w', encoding='utf-8', newline='\n').write(d)
print('six fixes applied')
