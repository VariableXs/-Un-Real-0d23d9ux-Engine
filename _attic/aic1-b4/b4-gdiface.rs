
// ---------------------------------------------------------------------------
// F006 · 深化批次四：文本绘制字形图集热路径记账（F055 缓存联动）
//
// 主册依据（G-A-06【设计细节】）：「文本绘制走字形图集热路径（F055 缓存）」
// ——热路径不是口号：每笔文本绘制按字形是否在图集记账，命中率可审计
// （F055 判据线 >90% 场景值的本域观测面）。
// ---------------------------------------------------------------------------

/// 字形图集路径记账（TextOut/ExtTextOut/DrawText 共用）。
#[derive(Clone, Copy, Debug)]
pub struct GlyphPathLedger {
    /// 图集命中字形数（热路径）。
    pub atlas_hits: u64,
    /// 图集未命中字形数（冷路径——光栅化后按 F055 语义回填图集）。
    pub atlas_misses: u64,
    /// 回填图集次数（未命中后的回填——miss 与 refill 差值 = 回填被拒数，
    /// 如实可见）。
    pub refills: u64,
}

impl GlyphPathLedger {
    pub const fn new() -> GlyphPathLedger {
        GlyphPathLedger { atlas_hits: 0, atlas_misses: 0, refills: 0 }
    }

    /// 一笔字形绘制：命中走热路径；未命中记账并尝试回填（回填结果如实分账）。
    pub fn note_glyph(&mut self, in_atlas: bool, refill_ok: bool) {
        if in_atlas {
            self.atlas_hits += 1;
        } else {
            self.atlas_misses += 1;
            if refill_ok {
                self.refills += 1;
            }
        }
    }

    /// 热路径命中率（permille）；零字形 → None（不猜）。
    pub fn hit_permille(&self) -> Option<u32> {
        let total = self.atlas_hits + self.atlas_misses;
        if total == 0 {
            return None;
        }
        Some((self.atlas_hits * 1000 / total) as u32)
    }
}

/// F006 深化批次四自检。
pub fn run_gdiface_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F006-gdiface-deep3");
    // 1) 记账分面：命中/未命中/回填三计数独立；恒等式 hits+misses = 总字形。
    let mut led = GlyphPathLedger::new();
    for i in 0..10u64 {
        led.note_glyph(i < 8, true);
    }
    cs.add(
        "glyph_ledger_facets",
        led.atlas_hits == 8 && led.atlas_misses == 2 && led.refills == 2 && led.hit_permille() == Some(800),
        "",
    );
    // 2) 回填被拒如实可见（refill 恒等破缺——不给「都进图集了」假象）。
    let mut led2 = GlyphPathLedger::new();
    led2.note_glyph(false, false);
    led2.note_glyph(false, true);
    cs.add(
        "glyph_refill_refusal_visible",
        led2.atlas_misses == 2 && led2.refills == 1,
        "",
    );
    // 3) 零字形命中率如实 None（不猜）。
    cs.add("glyph_ledger_empty_none", GlyphPathLedger::new().hit_permille().is_none(), "");
    cs
}
