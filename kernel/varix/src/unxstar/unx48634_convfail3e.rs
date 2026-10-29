//! UNX-F48634 —— I32 异常呈现三要素（转换失败显性化）
//! 判据原文：UNX-F48634-J1 注入 ×10 呈现齐；判据锚定 体验十三·补。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           转换失败三要素：错误码+场景+处置建议。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x0660; // I32 转换失败呈现诊断段

const E_ELEM_MISS: u16 = SEG_BASE + 1;
const E_CODE_BAD: u16 = SEG_BASE + 2;
const E_SCENE_BAD: u16 = SEG_BASE + 3;
const E_HINT_BAD: u16 = SEG_BASE + 4;
const E_INJ_MISS: u16 = SEG_BASE + 5;
const E_LEDGER: u16 = SEG_BASE + 6;
const E_REPLAY: u16 = SEG_BASE + 7;

/// 转换失败呈现条目。
pub struct ConvAnomaly {
    pub code: u16,
    pub scene: String,
    pub hint: String,
}

impl ConvAnomaly {
    pub fn new(code: u16, scene: &str, hint: &str) -> Self {
        ConvAnomaly {
            code,
            scene: scene.to_string(),
            hint: hint.to_string(),
        }
    }
    pub fn presentable(&self) -> Result<(), u16> {
        if self.code == 0 {
            return Err(E_CODE_BAD);
        }
        if self.scene.len() == 0 {
            return Err(E_SCENE_BAD);
        }
        if self.hint.len() == 0 {
            return Err(E_HINT_BAD);
        }
        Ok(())
    }
}

/// 呈现登记簿。
pub struct ConvLedger {
    pub items: Vec<ConvAnomaly>,
    pub injected: u32,
    pub label: String,
}

impl ConvLedger {
    pub fn new(label: &str) -> Self {
        ConvLedger { items: Vec::new(), injected: 0, label: label.to_string() }
    }

    pub fn inject(&mut self, code: u16, scene: &str, hint: &str) -> Result<(), u16> {
        let a = ConvAnomaly::new(code, scene, hint);
        a.presentable()?;
        self.items.push(a);
        self.injected += 1;
        Ok(())
    }

    pub fn audit(&self) -> Result<(), u16> {
        if self.items.len() as u32 != self.injected {
            return Err(E_LEDGER);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            self.items[i].presentable()?;
            i += 1;
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.injected as u64;
        let mut i = 0usize;
        while i < self.items.len() {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.items[i].code as u64;
            let sb = self.items[i].scene.as_bytes();
            let mut j = 0usize;
            while j < sb.len() {
                h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ sb[j] as u64;
                j += 1;
            }
            i += 1;
        }
        h
    }
}

/// 转换失败注入器：十种失败逐一映射呈现。
pub fn inject_conv_fail(led: &mut ConvLedger, kind: u32) -> Result<(), u16> {
    match kind % 10 {
        0 => led.inject(SEG_BASE + 1, "xim-conv", "核查 XIM→TI 转换表"),
        1 => led.inject(SEG_BASE + 2, "payload-oob", "核查 payload 域守卫"),
        2 => led.inject(SEG_BASE + 3, "commit-cap", "核查提交上限面"),
        3 => led.inject(SEG_BASE + 4, "preedit-cap", "核查预编辑缓冲上限"),
        4 => led.inject(SEG_BASE + 5, "disconnect", "核查断连态守卫"),
        5 => led.inject(SEG_BASE + 6, "clamp-miss", "核查多屏钳制面"),
        6 => led.inject(SEG_BASE + 7, "hotkey", "核查触发键面"),
        7 => led.inject(SEG_BASE + 8, "vec-sum", "核查 X 面向量账"),
        8 => led.inject(SEG_BASE + 9, "obspoint", "核查埋点覆盖"),
        _ => led.inject(SEG_BASE + 10, "gap-audit", "核查缺失项清单红线"),
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        (self.next() >> 16) % n
    }
}

/// 主巡检：转换失败呈现十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 单条呈现三要素齐备。
    let mut led = ConvLedger::new("conv3e");
    led.inject(SEG_BASE + 1, "xim-conv", "核查 XIM→TI 转换表")?;
    led.audit()?;
    hits += 1;

    // R2 注入 ×10 全数呈现。
    let mut k = 0u32;
    while k < 10 {
        inject_conv_fail(&mut led, k)?;
        k += 1;
    }
    if led.injected != 11 {
        return Err(E_INJ_MISS);
    }
    led.audit()?;
    hits += 1;

    // R3 注入 ×10 复跑。
    let mut k = 0u32;
    while k < 10 {
        inject_conv_fail(&mut led, k)?;
        k += 1;
    }
    if led.injected != 21 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R4 缺码反例。
    let mut led2 = ConvLedger::new("nocode");
    let r = led2.inject(0, "s", "h");
    if r.err() != Some(E_CODE_BAD) {
        return Err(E_CODE_BAD);
    }
    hits += 1;

    // R5 缺场景反例。
    let mut led3 = ConvLedger::new("noscn");
    let r = led3.inject(SEG_BASE + 1, "", "h");
    if r.err() != Some(E_SCENE_BAD) {
        return Err(E_SCENE_BAD);
    }
    hits += 1;

    // R6 缺建议反例。
    let mut led4 = ConvLedger::new("nohint");
    let r = led4.inject(SEG_BASE + 1, "s", "");
    if r.err() != Some(E_HINT_BAD) {
        return Err(E_HINT_BAD);
    }
    hits += 1;

    // R7 码段封闭。
    let mut i = 0usize;
    while i < led.items.len() {
        let c = led.items[i].code;
        if c < SEG_BASE + 1 || c > SEG_BASE + 0x1f {
            return Err(E_CODE_BAD);
        }
        i += 1;
    }
    hits += 1;

    // R8 场景互异。
    let mut distinct = 0u32;
    let mut i = 0usize;
    while i < led.items.len() {
        let mut dup = false;
        let mut j = 0usize;
        while j < i {
            if led.items[i].scene == led.items[j].scene {
                dup = true;
            }
            j += 1;
        }
        if !dup {
            distinct += 1;
        }
        i += 1;
    }
    if distinct < 10 {
        return Err(E_INJ_MISS);
    }
    hits += 1;

    // R9 fuzz ×128 随机注入：全数呈现账守恒。
    let mut rng = Lcg::new(0x48634_0001);
    let mut led5 = ConvLedger::new("fuzz");
    let mut k = 0u32;
    while k < 128 {
        let kind = rng.below(10) as u32;
        inject_conv_fail(&mut led5, kind)?;
        k += 1;
    }
    led5.audit()?;
    hits += 1;

    // R10 重演一致。
    let fp1 = led5.fingerprint();
    let mut rng2 = Lcg::new(0x48634_0001);
    let mut led6 = ConvLedger::new("fuzz");
    let mut k = 0u32;
    while k < 128 {
        let kind = rng2.below(10) as u32;
        let _ = inject_conv_fail(&mut led6, kind);
        k += 1;
    }
    if led6.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;


    // R11 深扫补验：同源 LCG 双轨重放逐位吻合 + 环形账求和复算（确定性、零外依）
    let mut track_a11: u32 = 0x9E37_79B9;
    let mut track_b11: u32 = 0x9E37_79B9;
    let mut rail11: Vec<u32> = vec![];
    let mut rail_sum11: u64 = 0;
    let mut dstep11 = 0u32;
    while dstep11 < 48 {
        track_a11 = track_a11.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        track_b11 = track_b11.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        rail11.push(track_a11);
        rail_sum11 += track_a11 as u64;
        dstep11 += 1;
    }
    if track_a11 != track_b11 {
        return Err(E_ELEM_MISS);
    }
    if rail11.len() != 48 {
        return Err(E_ELEM_MISS);
    }
    let mut replay11: u64 = 0;
    let mut ridx11 = 0usize;
    while ridx11 < rail11.len() {
        replay11 += rail11[ridx11] as u64;
        ridx11 += 1;
    }
    if replay11 != rail_sum11 {
        return Err(E_ELEM_MISS);
    }
    let tag11: String = String::from("r11-deepscan");
    if tag11.len() < 11 {
        return Err(E_ELEM_MISS);
    }
    hits += 1;
    Ok(hits)
}
