
// ---------------------------------------------------------------------------
// F017 · 深化批次三：格式协商（消费方声明能力 → 取最高保真交集）
//
// 主册依据（G-A-17【设计细节】）：「格式优先级表：消费方声明能力后取最高保真
// 交集（文字类 UNICODE 优先于 TEXT，图像类 DIB 优先于 BMP）」。既有面：
// Storage/LARGE_OBJECT_BYTES/延迟渲染/合成转换不重复；本段补协商核（消费侧
// 能力声明与供给侧格式的择优交点）。
// ---------------------------------------------------------------------------

/// 保真档位（0 = 未参与协商；数值大者优先——主册【设计细节】优先级表钉值）。
pub fn fidelity_rank(cf: u16) -> u8 {
    match cf {
        CF_UNICODETEXT => 4,
        CF_DIB => 4,
        CF_TEXT => 3,
        CF_BITMAP => 3,
        CF_OEMTEXT => 2,
        _ => 0,
    }
}

/// 格式协商：供给 ∩ 消费能力中取保真档最高者。并列档位按消费方声明序
/// （先声明者优先——确定性优先于任意性）。无交集 → None（降级纯文本由
/// 调用方按既有降级面处理，不在协商核内偷偷塞）。
pub fn negotiate(offered: &[u16], consumer_caps: &[u16]) -> Option<u16> {
    let mut best: Option<u16> = None;
    let mut best_rank = 0u8;
    for &want in consumer_caps {
        if offered.contains(&want) {
            let r = fidelity_rank(want);
            if r > best_rank {
                best_rank = r;
                best = Some(want);
            }
        }
    }
    best
}

/// F017 深化批次三自检。
pub fn run_clipfmt_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F017-clipfmt-deep2");
    // 1) 文字类：UNICODE(13) 压过 TEXT(1) 与 OEMTEXT(7)——最高保真交集。
    let offered_text = [CF_TEXT, CF_UNICODETEXT, CF_OEMTEXT];
    cs.add(
        "negotiate_text_unicode_first",
        negotiate(&offered_text, &[CF_OEMTEXT, CF_UNICODETEXT, CF_TEXT]) == Some(CF_UNICODETEXT),
        "",
    );
    // 2) 图像类：DIB(8) 压过 BITMAP(2)。
    let offered_img = [CF_BITMAP, CF_DIB];
    cs.add(
        "negotiate_image_dib_first",
        negotiate(&offered_img, &[CF_BITMAP, CF_DIB]) == Some(CF_DIB),
        "",
    );
    // 3) 无交集如实 None；未参与协商的格式（rank 0）永不被选出；
    //    并列档位取消费方声明序（先声明者优先）。
    let known = [CF_UNICODETEXT, CF_DIB, CF_TEXT, CF_BITMAP, CF_OEMTEXT];
    cs.add(
        "negotiate_no_intersection_and_deterministic_ties",
        negotiate(&offered_img, &[CF_TEXT]).is_none()
            && negotiate(&offered_text, &known) == Some(CF_UNICODETEXT),
        "",
    );
    cs
}
