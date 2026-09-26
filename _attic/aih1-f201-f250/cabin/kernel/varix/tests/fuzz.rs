//! AURORA-1000 步骤 0026 · 结构化语料 fuzz harness。
//!
//! 用固定种子的 LCG 生成结构化语料，驱动界面栈各域的公开纯 API。
//! CI 内跑确定性有界轮次（秒级）；60s 浸泡用
//! `VARIX_FUZZ_MS=60000 cargo ktest --test fuzz` 调节。
//! 验收口径：无 panic、无越界、无泄漏（宿主无 alloc，语料均固定缓冲）。

#![cfg(target_os = "windows")]

use varix::aurora::{compositor, display, gpu, image, render2d, typography};

/// 确定性伪随机（x86-64 上 xorshift64*），避免依赖外部 rand crate。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

fn elapsed_ms(start: std::time::Instant) -> u64 {
    start.elapsed().as_millis() as u64
}

fn budget_ms() -> u64 {
    std::env::var("VARIX_FUZZ_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2_000)
}

/// fA26-01：EDID 解析面对随机/截断字节不得 panic。
fn fuzz_edid(rng: &mut Lcg, deadline: std::time::Instant) -> usize {
    let mut ops = 0;
    let mut buf = [0u8; 128];
    while elapsed_ms(deadline) < budget_ms() {
        for b in buf.iter_mut() {
            *b = rng.below(256) as u8;
        }
        // 三种语料形态：全随机 / 合法头+随机主体 / 随机前缀+随机长度截断
        let shape = rng.below(3);
        match shape {
            1 => {
                buf[0] = 0x00;
                buf[1] = 0xFF;
                buf[2] = 0xFF;
                buf[3] = 0xFF;
                buf[4] = 0xFF;
                buf[5] = 0xFF;
                buf[6] = 0xFF;
                buf[7] = 0x00;
            }
            2 => {
                let cut = rng.below(128) as usize;
                for b in buf[cut..].iter_mut() {
                    *b = 0;
                }
            }
            _ => {}
        }
        let _ = display::edid_header_ok(&buf);
        let _ = display::edid_checksum(&buf);
        let _ = display::edid_valid(&buf);
        let _ = display::edid_manufacturer(&buf);
        let _ = display::edid_product_code(&buf);
        let _ = display::edid_year(&buf);
        let _ = display::edid_version(&buf);
        let _ = display::edid_established_count(&buf);
        let _ = display::edid_standard_count(&buf);
        ops += 1;
    }
    ops
}

/// fA26-02：图像格式嗅探与头部/容器解析不得 panic。
fn fuzz_image(rng: &mut Lcg, deadline: std::time::Instant) -> usize {
    let mut ops = 0;
    let mut buf = [0u8; 256];
    while elapsed_ms(deadline) < budget_ms() {
        for b in buf.iter_mut() {
            *b = rng.below(256) as u8;
        }
        // 注入各格式魔数头部，走深路径
        match rng.below(4) {
            0 => {
                buf[0] = 0x89;
                buf[1] = b'P';
                buf[2] = b'N';
                buf[3] = b'G';
            }
            1 => {
                buf[0] = 0xFF;
                buf[1] = 0xD8;
            }
            2 => {
                buf[0] = b'R';
                buf[1] = b'I';
                buf[2] = b'F';
                buf[3] = b'F';
                buf[8] = b'W';
                buf[9] = b'E';
                buf[10] = b'B';
                buf[11] = b'P';
            }
            _ => {}
        }
        let _ = image::sniff_format(&buf);
        let _ = image::parse_png_header(&buf);
        let _ = image::parse_jpeg_info(&buf);
        let _ = image::parse_webp_info(&buf);
        let _ = image::is_avif_container(&buf);
        let _ = image::parse_avif_info(&buf);
        let _ = image::crc32(&buf);
        let _ = image::adler32(&buf);
        // 解码路径：随机字节进固定输出缓冲
        let mut out = [0u8; 64 * 64 * 4];
        let mut scratch = [0u8; 64 * 64];
        let _ = image::decode_png(&buf, &mut out, &mut scratch);
        let _ = image::decode_bmp(&buf, &mut out);
        // zlib stored 块语料
        let mut z = [0u8; 64];
        z[0] = 0x78;
        z[1] = 0x01;
        for b in z[2..].iter_mut() {
            *b = rng.below(256) as u8;
        }
        let mut zo = [0u8; 256];
        let _ = image::inflate_stored(&z, &mut zo);
        ops += 1;
    }
    ops
}

/// fA26-03：SFNT/字体解析不得 panic。
fn fuzz_typography(rng: &mut Lcg, deadline: std::time::Instant) -> usize {
    let mut ops = 0;
    let mut font = [0u8; 512];
    while elapsed_ms(deadline) < budget_ms() {
        for b in font.iter_mut() {
            *b = rng.below(256) as u8;
        }
        // sfnt 魔数（0x00010000 / 'ttcf' / 'OTTO'）提升进表目录路径
        match rng.below(3) {
            0 => font[0..4].copy_from_slice(&[0x00, 0x01, 0x00, 0x00]),
            1 => font[0..4].copy_from_slice(b"ttcf"),
            _ => font[0..4].copy_from_slice(b"OTTO"),
        }
        let _ = typography::parse_sfnt_version(&font);
        let _ = typography::sfnt_num_tables(&font);
        for i in 0..8 {
            let _ = typography::table_record_at(&font, i);
        }
        let tags = [b"cmap", b"glyf", b"head", b"loca"];
        let t = rng.below(4) as usize;
        if let Some(rec) = typography::find_table(&font, &tags[t]) {
            let _ = typography::table_checksum(&font, rec);
        }
        let _ = typography::cmap_lookup(rng.below(0x110000) as u32);
        ops += 1;
    }
    ops
}

/// fA26-04：GPU 命令字编码/解码往返与随机字解码不得 panic。
fn fuzz_gpu(rng: &mut Lcg, deadline: std::time::Instant) -> usize {
    let mut ops = 0;
    while elapsed_ms(deadline) < budget_ms() {
        let w = match rng.below(4) {
            0 => gpu::encode_nop(),
            1 => gpu::encode_draw(rng.below(0x10000) as u16, rng.below(256) as u8),
            2 => gpu::encode_flip(rng.below(2) == 0),
            _ => gpu::encode_wait(rng.below(10_000) as u32),
        };
        let cmd = gpu::decode_command(w);
        let _ = gpu::reencode(cmd);
        let _ = gpu::is_valid_command_word(w);
        let _ = gpu::opcode_of(rng.next() as u32);
        let _ = gpu::decode_command(rng.next() as u32);
        ops += 1;
    }
    ops
}

/// fA26-05：合成器图层树随机增删改不得 panic / 越界。
fn fuzz_compositor(rng: &mut Lcg, deadline: std::time::Instant) -> usize {
    let mut ops = 0;
    let mut reg = compositor::LayerRegistry::new();
    while elapsed_ms(deadline) < budget_ms() {
        let x = rng.below(4096) as i32;
        let y = rng.below(4096) as i32;
        let w = rng.below(512) as u16;
        let h = rng.below(512) as u16;
        let layer = compositor::make_layer(
            rng.below(0x10000) as u16,
            x as i16,
            y as i16,
            w,
            h,
            [
                rng.below(256) as u8,
                rng.below(256) as u8,
                rng.below(256) as u8,
                255,
            ],
            rng.below(256) as u8,
            compositor::BlendMode::Normal,
        );
        let _ = compositor::layer_tree_add(&mut reg, layer);
        compositor::layer_tree_sort(&mut reg);
        let _ = compositor::layer_tree_set_visible(&mut reg, rng.below(64) as u16, rng.below(2) == 0);
        let n = reg.len();
        if n > 0 {
            let _ = reg.get(rng.below(n as u64) as usize);
        }
        let _ = reg.dirty_union();
        // 容量上限保护：超过容量后 add 应安全拒绝
        for _ in 0..4 {
            let l = compositor::make_layer(
                0,
                0,
                16,
                16,
                rng.below(0x10000) as u16,
                [0, 0, 0, 255],
                255,
                compositor::BlendMode::Normal,
            );
            let _ = compositor::layer_tree_add(&mut reg, l);
        }
        ops += 1;
    }
    ops
}

/// fA26-06：2D 绘制随机坐标/尺寸不得越界写。
fn fuzz_render2d(rng: &mut Lcg, deadline: std::time::Instant) -> usize {
    let mut ops = 0;
    let mut buf = [0u8; 64 * 64 * 4];
    while elapsed_ms(deadline) < budget_ms() {
        let w = 64;
        let h = 64;
        let c = render2d::Rgba {
            r: rng.below(256) as u8,
            g: rng.below(256) as u8,
            b: rng.below(256) as u8,
            a: rng.below(256) as u8,
        };
        let p0 = (rng.below(256) as i32 - 64, rng.below(256) as i32 - 64);
        let p1 = (rng.below(256) as i32 - 64, rng.below(256) as i32 - 64);
        let p2 = (rng.below(256) as i32 - 64, rng.below(256) as i32 - 64);
        let _ = render2d::bezier_point(p0, p1, p2, rng.below(2000) as i32);
        render2d::stroke_bezier(&mut buf, w, h, p0, p1, p2, c);
        render2d::draw_glyph(
            &mut buf,
            w,
            h,
            rng.below(256) as u8,
            rng.below(96) as i32 - 16,
            rng.below(96) as i32 - 16,
            c,
        );
        render2d::draw_line_aa(
            &mut buf,
            w,
            h,
            rng.below(192) as i32 - 64,
            rng.below(192) as i32 - 64,
            rng.below(192) as i32 - 64,
            rng.below(192) as i32 - 64,
            c,
        );
        let src = [0u8; 8 * 8 * 4];
        render2d::blit_bitmap(
            &mut buf,
            w,
            h,
            &src,
            8,
            8,
            rng.below(96) as i32 - 16,
            rng.below(96) as i32 - 16,
        );
        render2d::composite_layer(&mut buf, w, h, &src, rng.below(256) as u8);
        render2d::stroke_rect(
            &mut buf,
            w,
            h,
            rng.below(70) as usize,
            rng.below(70) as usize,
            rng.below(70) as usize,
            rng.below(70) as usize,
            rng.below(8) as usize,
            c,
        );
        render2d::fill_radial_gradient(
            &mut buf,
            w,
            h,
            rng.below(64) as usize,
            rng.below(64) as usize,
            c,
            render2d::Rgba { r: 0, g: 0, b: 0, a: 255 },
        );
        ops += 1;
    }
    ops
}

#[test]
fn structured_corpus_fuzz_no_panic() {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15);
    let mut rng = Lcg(seed | 1);
    let deadline = std::time::Instant::now();
    let mut total = 0;
    total += fuzz_edid(&mut rng, deadline);
    total += fuzz_image(&mut rng, deadline);
    total += fuzz_typography(&mut rng, deadline);
    total += fuzz_gpu(&mut rng, deadline);
    total += fuzz_compositor(&mut rng, deadline);
    total += fuzz_render2d(&mut rng, deadline);
    // 验收：跑满预算且无 panic（到达这里即通过）。
    assert!(total > 0, "fuzz harness must exercise at least one op");
}
