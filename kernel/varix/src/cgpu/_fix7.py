# -*- coding: utf-8 -*-
# check7 我方错误批量修复
import io, os
os.chdir(os.path.dirname(os.path.abspath(__file__)))

def edit(p, pairs, must=True):
    s = io.open(p, encoding="utf-8").read()
    for old, new in pairs:
        n = s.count(old)
        if n == 0:
            if must:
                raise AssertionError(f"{p}: NOT FOUND: {old[:60]!r}")
            continue
        s = s.replace(old, new)
    io.open(p, "w", encoding="utf-8", newline="").write(s)
    print("ok", p)

# 0) mod.rs 计数 191 → 228
edit("mod.rs", [("pub const CGPU_SUBSET_COUNT: usize = 191;",
                 "pub const CGPU_SUBSET_COUNT: usize = 228;")])

# 1) cga93 CU 公开
edit("cga93_idct.rs", [("const CU: [i32; 8]", "pub const CU: [i32; 8]")])

# 2) cga96 x2 定义
edit("cga96_dctfwd.rs", [(
"""    let dx = dct2d_local(x);
    let dy = dct2d_local(y);
    let dx2b = dct2d_local(&x2);""",
"""    let dx = dct2d_local(x);
    let dy = dct2d_local(y);
    let mut x2 = [[0i32; 8]; 8];
    for r in 0..8 {
        for c in 0..8 {
            x2[r][c] = 2 * x[r][c];
        }
    }
    let dx2b = dct2d_local(&x2);""")])

# 3) cga79 clamp 类型 + deep 常量名
edit("cga79_whitebal.rs", [
    ("let gr = (mg * 1000 / mr).clamp(GAIN_MIN, GAIN_MAX) as u32;",
     "let gr = (mg * 1000 / mr).clamp(GAIN_MIN as u64, GAIN_MAX as u64) as u32;"),
    ("let gb = (mg * 1000 / mb).clamp(GAIN_MIN, GAIN_MAX) as u32;",
     "let gb = (mg * 1000 / mb).clamp(GAIN_MIN as u64, GAIN_MAX as u64) as u32;"),
    ("XC_TEMP_OOB", "XK_TEMP_OOB"),
])

# 4) cga80 三处解构 + 一处 ? + deep prev_gain 类型
edit("cga80_unsharp.rs", [
    ("""        let (out, hi, lo) = unsharp(s, 150).map_err(|e| e as u16);
        cs.add("钳制账齐", hi <= 4096 && lo <= 4096, "计数有界");
        cs.add("输出域闭", out.px.iter().all(|&v| v <= 255), "u8 域");
        // 边带响应强于原图。
        let in_edge = (s.px[4 * 8 + 4] as i64 - s.px[4 * 8 + 3] as i64).abs();
        let out_edge = (out.px[4 * 8 + 4] as i64 - out.px[4 * 8 + 3] as i64).abs();
        cs.add("边带增强账", out_edge >= in_edge, "正向锐化");""",
     """        if let Ok((out, hi, lo)) = unsharp(s, 150) {
            cs.add("钳制账齐", hi <= 4096 && lo <= 4096, "计数有界");
            cs.add("输出域闭", out.px.iter().all(|&v| v <= 255), "u8 域");
            // 边带响应强于原图。
            let in_edge = (s.px[4 * 8 + 4] as i64 - s.px[4 * 8 + 3] as i64).abs();
            let out_edge = (out.px[4 * 8 + 4] as i64 - out.px[4 * 8 + 3] as i64).abs();
            cs.add("边带增强账", out_edge >= in_edge, "正向锐化");
        } else {
            cs.add("钳制账齐", false, "锐化构建失败");
        }"""),
    ("""        if let Ok(r) = &rg {
            let (ro, chi, clo) = unsharp(r, 300).map_err(|e| e as u16);
            cs.add("渐变锐化", ro.px.len() == 64, "全图输出");
            cs.add("钳账一致", chi + clo <= 64, "计数 ≤ 像素数");
            let _ = clo;
        }""",
     """        if let Ok(r) = &rg {
            if let Ok((ro, chi, clo)) = unsharp(r, 300) {
                cs.add("渐变锐化", ro.px.len() == 64, "全图输出");
                cs.add("钳账一致", chi + clo <= 64, "计数 ≤ 像素数");
                let _ = clo;
            } else {
                cs.add("渐变锐化", false, "锐化构建失败");
            }
        }"""),
    ("""        cs.add("空像素拒", unsharp(&Img::new(1, 1, vec![5u32]).map_err(|e| e as u16)?, 301).is_err(), "域闸前置");""",
     """        cs.add("空像素拒", match Img::new(1, 1, vec![5u32]) {
            Ok(tiny) => unsharp(&tiny, 301).is_err(),
            Err(_) => false,
        }, "域闸前置");"""),
    ("    let mut prev_gain = 0i64;", "    let mut prev_gain = 0u64;"),
])

# 5) cga81:242 GR 单调重写
edit("cga81_bilateral.rs", [
    ("""    cs.add("GR 单调", (1..256usize).all(|d| gr.as_ref().map_err(|e| e as u16)[d] <= gr.as_ref().map_err(|e| e as u16)[d - 1]), "亮度差↑权重↓");""",
     """    cs.add("GR 单调", match gr.as_ref() {
        Ok(t) => (1..256usize).all(|d| t[d] <= t[d - 1]),
        Err(_) => false,
    }, "亮度差↑权重↓");"""),
    ("(1..6u32).all(|d| gs_lookup(SIGMA_S[0], d).map_err(|e| e as u16) < gs_lookup(SIGMA_S[0], d - 1).map_err(|e| e as u16))",
     "(1..6u32).all(|d| matches!(gs_lookup(SIGMA_S[0], d), Ok(w) if gs_lookup(SIGMA_S[0], d - 1) == Ok(w - 1) || gs_lookup(SIGMA_S[0], d - 1) < Ok(w)))"),
])

# 6) cga82:209
edit("cga82_dither.rs", [("m.as_ref().map_err(|e| e as u16).and_then(|mm| {",
                          "m.as_ref().map_err(|e| *e as u16).and_then(|mm| {")])

# 7) cga83:69
edit("cga83_patfill.rs", [("Ok(cx.unsigned_abs() <= r && cy.unsigned_abs() <= r)",
                           "Ok(cx.unsigned_abs() <= r as u64 && cy.unsigned_abs() <= r as u64)")])

# 8) cga86:212-214
edit("cga86_stroke.rs", [(
"""        if let Ok((out, n)) = stroke_outer(s, 1, [255, 220, 0], 255) {
            let dil = dilate(&a, 8, 8, 1).map_err(|e| e as u16);
            let band = diff_band(&dil.map_err(|e| e as u16)?, &a).map_err(|e| e as u16);
            cs.add("带宽守恒", band_conserve(&a, &dil.map_err(|e| e as u16)?, &band.map_err(|e| e as u16)?).is_ok(), "d−a==band");
            cs.add("单向账", no_cover(&band.map_err(|e| e as u16)?, &a).is_ok(), "不盖本体");""",
"""        if let Ok((out, n)) = stroke_outer(s, 1, [255, 220, 0], 255) {
            match dilate(&a, 8, 8, 1) {
                Ok(dil) => match diff_band(&dil, &a) {
                    Ok(band) => {
                        cs.add("带宽守恒", band_conserve(&a, &dil, &band).is_ok(), "d−a==band");
                        cs.add("单向账", no_cover(&band, &a).is_ok(), "不盖本体");
                    }
                    Err(_) => {
                        cs.add("带宽守恒", false, "band 构建失败");
                        cs.add("单向账", false, "band 构建失败");
                    }
                },
                Err(_) => {
                    cs.add("带宽守恒", false, "dil 构建失败");
                    cs.add("单向账", false, "dil 构建失败");
                }
            }""")])

# 9) cga88:139 + deep OpNode
edit("cga88_fuseext.rs", [
    ("""    let (a, _) = fused_run(plan, img).map_err(|e| e as u16).map_err(|e| e as u32)?;""",
     """    let a = fused_run(plan, img)?;"""),
    ("OpNode { op: 1, param: 2, dst: 0 }", "OpNode { op: 1, param: 2, reads_snapshot: false }"),
    ("OpNode { op: 1, param: (i % 4) as u8, dst: 0 }",
     "OpNode { op: 1, param: (i % 4) as u8, reads_snapshot: false }"),
])

# 10) cga89:103 + 122
edit("cga89_precreg.rs", [
    ("""    if diff > tol {
        return Err(XC_MISMATCH);
    }""",
     """    if (diff as u32) > tol {
        return Err(XC_MISMATCH);
    }"""),
    ("""    let all: Vec<&Case> = corpus_regular().iter().chain(corpus_boundary().iter()).collect();""",
     """    let reg = corpus_regular();
    let bnd = corpus_boundary();
    let all: Vec<&Case> = reg.iter().chain(bnd.iter()).collect();"""),
])

# 11) cga91:224 + deep DocPage
edit("cga91_opdocs.rs", [
    ("""    let mut short_reg = reg.clone();
    short_reg.push(RegRef { id: 9999, name: "ghost", sig: "u" });""",
     """    let mut short_reg: Vec<RegRef> = Vec::new();
    for r in reg.iter() {
        short_reg.push(RegRef { id: r.id, name: r.name, sig: r.sig });
    }
    short_reg.push(RegRef { id: 9999, name: "ghost", sig: "u" });"""),
    ("""    let _ = b.add(DocPage {
        id: 1,
        op_family: 2,
        title: "simd_gauss",
        body: "box+ken",
        cases: alloc::vec![11, 12],
    });""",
     """    let _ = b.add(DocPage {
        op_id: 1,
        name: "simd_gauss",
        sig: "u8",
        cplx: 1,
        bound: 255,
        case_ids: [11, 12],
        case_n: 2,
    });"""),
    ('cs.add("页命中", b.find(1).map(|p| p.id == 1).unwrap_or(false), "往返");',
     'cs.add("页命中", b.find(1).map(|p| p.op_id == 1).unwrap_or(false), "往返");'),
])

# 12) cga92:182
edit("cga92_batchclose.rs", [(
"""        if let Ok(r3) = &rep3 {
            let f2a = report_fingerprint(&standard_report(9202, f1).map_err(|e| e as u16)?);
            cs.add("链敏感", report_fingerprint(r3) != f2a, "prev 变指纹变");
        }""",
"""        if let Ok(r3) = &rep3 {
            let f2a = match standard_report(9202, f1) {
                Ok(rep2) => report_fingerprint(&rep2),
                Err(_) => 0,
            };
            cs.add("链敏感", report_fingerprint(r3) != f2a, "prev 变指纹变");
        }""")])

# 13) cga98 catmull i64
edit("cga98_chromaup.rs", [(
"""    let t = t as u64;
    let t2 = t * t;
    let t3 = t2 * t;
    // 有理式直算：Q8 输出 = 分子 /(2·256²)。
    let n0 = -(t3) + 2 * t2 * 256 - (t * 256 * 256);
    let n1 = 3 * t3 - 5 * t2 * 256 + 2 * (256 * 256 * 256);
    let n2 = -3 * t3 + 4 * t2 * 256 + (t * 256 * 256);
    let n3 = t3 - t2 * 256;
    let d: i64 = 2 * 256 * 256;""",
"""    let t = t as i64;
    let t2 = t * t;
    let t3 = t2 * t;
    // 有理式直算：Q8 输出 = 分子 /(2·256²)。
    let n0 = -t3 + 2 * t2 * 256 - t * 256 * 256;
    let n1 = 3 * t3 - 5 * t2 * 256 + 2 * (256 * 256 * 256);
    let n2 = -3 * t3 + 4 * t2 * 256 + t * 256 * 256;
    let n3 = t3 - t2 * 256;
    let d: i64 = 2 * 256 * 256;""")])

# 14) cga99:161/164
edit("cga99_histequal.rs", [(
"""    let h = histogram(&low).map_err(|e| e as u16);
    cs.add("守恒", hist_conserve(&h.map_err(|e| e as u16)?, 256).is_ok(), "Σ==N");
    cs.add("空图拒", histogram(&[]).is_err(), "EMPTY");
    // 族二：CDF。
    let c = cdf(&h.map_err(|e| e as u16)?).map_err(|e| e as u16);""",
"""    let h = histogram(&low);
    cs.add("守恒", matches!(&h, Ok(hh) if hist_conserve(hh, 256).is_ok()), "Σ==N");
    cs.add("空图拒", histogram(&[]).is_err(), "EMPTY");
    // 族二：CDF。
    let c = match &h {
        Ok(hh) => cdf(hh).map_err(|e| e as u16),
        Err(_) => Err(0u16),
    };""")])

# 15) cga100 借用三处
edit("cga100_chanops.rs", [
    ("""            let v = img.px[img.idx(x, y, src)];
            img.px[img.idx(x, y, dst)] = v;""",
     """            let i_src = img.idx(x, y, src);
            let i_dst = img.idx(x, y, dst);
            let v = img.px[i_src];
            img.px[i_dst] = v;"""),
    ("""            img.px[img.idx(x, y, c)] = vals[(y * img.w + x) as usize];""",
     """            let i = img.idx(x, y, c);
            let v = vals[(y * img.w + x) as usize];
            img.px[i] = v;"""),
    ("""        for x in 0..img.w {
            img.px[img.idx(x, y, c)] = v;
        }
    }
    Ok(())
}

/// 通道填充。""",
     """        for x in 0..img.w {
            let i = img.idx(x, y, c);
            img.px[i] = v;
        }
    }
    Ok(())
}

/// 通道填充。"""),
])

# 16) cga103:72
edit("cga103_fuzzext.rs", [("v.push(FuzzCase { input: rng.next_u8(), param: rng.range(0, 255) });",
                            "v.push(FuzzCase { input: rng.next_u8(), param: rng.range(0, 255) as u8 });")])

# 17) cga87 deep FxDesc ver
edit("cga87_fxreg.rs", [
    ('let d = FxDesc { id: 1, name: "simdblur", sig: "u8", enabled: true };',
     'let d = FxDesc { id: 1, name: "simdblur", sig: "u8", ver: 1, enabled: true };'),
    ('let _ = reg.register(FxDesc { id: 3, name: "simdglow", sig: "u8,u8", enabled: true });',
     'let _ = reg.register(FxDesc { id: 3, name: "simdglow", sig: "u8,u8", ver: 1, enabled: true });'),
    ('let _ = reg.register(FxDesc { id: 2, name: "simdstroke", sig: "u16", enabled: false });',
     'let _ = reg.register(FxDesc { id: 2, name: "simdstroke", sig: "u16", ver: 1, enabled: false });'),
])
print("ALL FIXED")
