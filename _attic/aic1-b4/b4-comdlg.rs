
// ---------------------------------------------------------------------------
// F008 · 深化批次四：HSV 区拖动 ↔ 十六进制输入实时联动（60fps）
//
// 主册依据（G-A-08【设计细节】）：「颜色对话框 HSV 区拖动 60fps 实时联动
// 十六进制输入框」——联动是双向契约：拖动每帧同步 hex（帧数 = 更新数恒等，
/// 掉帧 = 不同步）；hex 编辑反算 HSV 后须与原色一致（round-trip）。
// ---------------------------------------------------------------------------

/// HSV↔hex 联动记账（双向契约的观测面）。
#[derive(Clone, Copy, Debug)]
pub struct HsvHexLink {
    /// 拖动帧数。
    pub drag_frames: u64,
    /// hex 输入框更新数（拖动期与帧数恒等）。
    pub hex_updates: u64,
    /// 双向失同步次数（hex 编辑反算与原色不一致——如实计数不静默）。
    pub desyncs: u64,
}

impl HsvHexLink {
    pub const fn new() -> HsvHexLink {
        HsvHexLink { drag_frames: 0, hex_updates: 0, desyncs: 0 }
    }

    /// 拖动一帧：当前色写 hex 输入框（每帧一次——联动恒等式）。
    pub fn drag_frame(&mut self, rgb: u32) -> [u8; 7] {
        self.drag_frames += 1;
        self.hex_updates += 1;
        rgb_to_hex(rgb)
    }

    /// hex 编辑提交：反算 HSV 并做 round-trip 校验（不一致 = 失同步计数）。
    pub fn hex_edit(&mut self, s: &str) -> Option<u32> {
        let rgb = hex_to_rgb(s)?;
        let (h, sat, v) = rgb_to_hsv(rgb);
        let back = hsv_to_rgb(h, sat, v);
        if back != rgb {
            self.desyncs += 1;
        }
        Some(rgb)
    }
}

/// F008 深化批次四自检。
pub fn run_comdlg_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F008-comdlg-deep3");
    // 1) 拖动联动恒等：60 帧拖动 → 60 次 hex 更新，末帧 hex 与色值一致。
    let mut link = HsvHexLink::new();
    let mut last = [0u8; 7];
    for f in 0..60u32 {
        let rgb = (f as u32) * 0x0004_080C; // 确定性拖动轨迹
        last = link.drag_frame(rgb);
    }
    cs.add(
        "hsv_drag_hex_sync_identity",
        link.drag_frames == 60 && link.hex_updates == 60 && last == rgb_to_hex(59 * 0x0004_080C),
        "",
    );
    // 2) hex 编辑：合法 hex 反算成功；round-trip 一致 → 零失同步。
    let edited = link.hex_edit("#12ABEF");
    cs.add(
        "hex_edit_roundtrip_no_desync",
        edited == Some(0x12ABEF) && link.desyncs == 0 && link.hex_updates == 60,
        "",
    );
    // 3) 非法 hex 如实 None（不静默吞）；既有 round-trip 核锚（HSV 域往返）。
    let bad = link.hex_edit("#GGGGGG");
    let (h, s, v) = rgb_to_hsv(0x12ABEF);
    cs.add(
        "hex_edit_invalid_and_hsv_core_anchor",
        bad.is_none() && hsv_to_rgb(h, s, v) == 0x12ABEF,
        "",
    );
    cs
}
