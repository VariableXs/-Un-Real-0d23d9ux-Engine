//! cgg10_nvapi — CGPU-F0970 NVIDIA 适配层 API 冻结（三组签名+同构对拍+十年承诺）。
//!
//! 判据原文（施工书锚点 CGPU-F0970）：API：NVIDIA 适配族冻结（NVIDIA 探测/
//! 差异表/媒体标签——签名冻结版本化）；与 F01/F02/F04 厂商适配层同构；十
//! 年承诺。判据：冻结、同构、承诺、判据。
//!
//! 职责定位：NVIDIA 适配族三组 API 冻结（nv_probe 探测/nv_diff 差异表/
//! nv_media_tag 媒体标签——每条签名=动词+参数形状+返回形状合成 FNV 指纹，
//! 注册即冻结，篡改即 0x7401 显性；重复注册 0x7403；冻结表只增不改，未开
//! 版本窗追加 0x7404——F0811 认证族冻结纪律同源）+同构对拍（与 F01/F02/
//! F04 厂商适配层同构：同动词集+同签名形状+同承诺域分隔模式——形状漂移
//! 0x7402，同构=跨厂商适配层可互换的机检面）+十年承诺（承诺域分隔
//! PROMISE_DOMAIN_NV 独立域签，承诺文本+签名合成总指纹——十年承诺失守
//! 0x7405）。诊断码段 0x7401~0x7405。

use alloc::vec::Vec;

/// 模块版本。
pub const NV_API_VERSION: &str = "G10-nvapi-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x74 段，本会话独占）
// ---------------------------------------------------------------------------

/// 0x7401 适配族签名失配。
pub const XC_NVAPI_SIG_TAMPER: u32 = 0x7401;
/// 0x7402 同构违约（与 F01/F02/F04 结构不同构）。
pub const XC_ISOMORPHISM_BROKEN: u32 = 0x7402;
/// 0x7403 重复注册。
pub const XC_NVAPI_DUP: u32 = 0x7403;
/// 0x7404 冻结表非法追加（未开版本窗）。
pub const XC_NVAPI_APPEND_ILLEGAL: u32 = 0x7404;
/// 0x7405 十年承诺失守（承诺指纹变）。
pub const XC_DECADE_PROMISE_BROKEN: u32 = 0x7405;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// FNV 偏移基（签名指纹种子——与 cgf13/cgd12 同源）。
pub const NVAPI_FNV_SEED: u64 = 0xC0FF_EE00_0000_0A70;
/// 承诺域分隔（PROMISE_DOMAIN_NV——与 PROMISE_DOMAIN_CERT/TIMER 防混签）。
pub const PROMISE_DOMAIN_NV: u32 = 0x4E56_5052; // "NVPR"
/// 适配族三组 API 数。
pub const NVAPI_GROUP_COUNT: usize = 3;
/// 同构对拍厂商数（F01/F02/F04 + NVIDIA 自身）。
pub const VENDOR_ISOMORPH_COUNT: usize = 4;

// ---------------------------------------------------------------------------
// 一、适配族签名（动词+形状→FNV 指纹）
// ---------------------------------------------------------------------------

/// API 签名（冻结原子单位）。
#[derive(Clone, Copy, Debug)]
pub struct NvApiSig {
    /// 动词（小写下划线——nv_probe/nv_diff/nv_media_tag）。
    pub verb: [u8; 16],
    /// 动词有效长度。
    pub verb_len: usize,
    /// 参数形状码（同形状=同码——同构对拍依据）。
    pub param_shape: u32,
    /// 返回形状码。
    pub ret_shape: u32,
    /// 冻结指纹（注册时合成，冻结后篡改即显性）。
    pub fingerprint: u64,
}

/// 形状码闭集（同构对拍口径：同形状=跨厂商同语义签名形状）。
pub mod shape {
    /// 探测形状：() -> 探测报告。
    pub const PROBE: u32 = 0x5052_4F42; // "PROB"
    /// 差异表形状：(能力键) -> 差异值。
    pub const DIFF: u32 = 0x4449_4646; // "DIFF"
    /// 媒体标签形状：(媒体流键) -> 标签集。
    pub const MEDIA_TAG: u32 = 0x4D54_4147; // "MTAG"
}

/// FNV-1a 步进（签名合成共用）。
const fn fnv_step(h: u64, byte: u8) -> u64 {
    (h ^ byte as u64).wrapping_mul(0x0100_0000_01B3)
}

/// 签名合成指纹：动词字节+形状码入 FNV。
pub const fn sig_fingerprint(verb: &[u8], param_shape: u32, ret_shape: u32) -> u64 {
    let mut h = NVAPI_FNV_SEED;
    let mut i = 0;
    while i < verb.len() {
        h = fnv_step(h, verb[i]);
        i += 1;
    }
    // 形状码 4 字节小端入签。
    h = fnv_step(h, (param_shape & 0xFF) as u8);
    h = fnv_step(h, ((param_shape >> 8) & 0xFF) as u8);
    h = fnv_step(h, ((param_shape >> 16) & 0xFF) as u8);
    h = fnv_step(h, ((param_shape >> 24) & 0xFF) as u8);
    h = fnv_step(h, (ret_shape & 0xFF) as u8);
    h = fnv_step(h, ((ret_shape >> 8) & 0xFF) as u8);
    h = fnv_step(h, ((ret_shape >> 16) & 0xFF) as u8);
    h = fnv_step(h, ((ret_shape >> 24) & 0xFF) as u8);
    h
}

/// 适配族注册表（判据"NVIDIA 探测/差异表/媒体标签——签名冻结版本化"）。
#[derive(Debug)]
pub struct NvApiRegistry {
    /// 冻结签名表。
    pub sigs: Vec<NvApiSig>,
    /// 注册表版本（冻结窗版本化）。
    pub version: u32,
    /// 篡改事件数（0x7401）。
    pub tamper_events: u32,
    /// 重复注册事件数（0x7403）。
    pub dup_events: u32,
    /// 非法追加事件数（0x7404）。
    pub illegal_append_events: u32,
}

impl NvApiRegistry {
    /// 新注册表（v1 冻结窗）。
    pub const fn new() -> NvApiRegistry {
        NvApiRegistry {
            sigs: Vec::new(),
            version: 1,
            tamper_events: 0,
            dup_events: 0,
            illegal_append_events: 0,
        }
    }

    /// 注册（冻结）：动词+形状→指纹入表。同动词重复注册 0x7403。
    pub fn register(
        &mut self,
        verb: &[u8],
        verb_len: usize,
        param_shape: u32,
        ret_shape: u32,
    ) -> Result<u64, u32> {
        // 重复注册检查（同动词+同参数形状=重复）。
        let mut i = 0;
        while i < self.sigs.len() {
            let s = &self.sigs[i];
            if s.param_shape == param_shape && s.verb_len == verb_len {
                let mut same = true;
                let mut j = 0;
                while j < verb_len.min(16) {
                    if s.verb[j] != verb[j] {
                        same = false;
                        j = 16;
                    }
                    j += 1;
                }
                if same {
                    self.dup_events += 1;
                    return Err(XC_NVAPI_DUP);
                }
            }
            i += 1;
        }
        let fp = sig_fingerprint(verb, param_shape, ret_shape);
        let mut v = [0u8; 16];
        let mut k = 0;
        while k < verb_len.min(16) {
            v[k] = verb[k];
            k += 1;
        }
        self.sigs.push(NvApiSig {
            verb: v,
            verb_len,
            param_shape,
            ret_shape,
            fingerprint: fp,
        });
        Ok(fp)
    }

    /// 冻结校验：表内逐条重算指纹，失配即 0x7401（篡改显性）。
    pub fn verify_frozen(&mut self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.sigs.len() {
            let s = &self.sigs[i];
            let want = sig_fingerprint(&s.verb, s.param_shape, s.ret_shape);
            if want != s.fingerprint {
                self.tamper_events += 1;
                return Err(XC_NVAPI_SIG_TAMPER);
            }
            i += 1;
        }
        Ok(())
    }

    /// 版本窗追加（合法路径：版本+1 再注册；未 bump 版本直接注册=0x7404）。
    pub fn open_window_append(&mut self, verb: &[u8], verb_len: usize, param_shape: u32, ret_shape: u32, bump: bool) -> Result<u64, u32> {
        if !bump {
            self.illegal_append_events += 1;
            return Err(XC_NVAPI_APPEND_ILLEGAL);
        }
        self.version += 1;
        self.register(verb, verb_len, param_shape, ret_shape)
    }
}

// ---------------------------------------------------------------------------
// 二、同构对拍（与 F01/F02/F04 厂商适配层）
// ---------------------------------------------------------------------------

/// 厂商适配层形状画像（同构对拍输入）。
#[derive(Clone, Copy, Debug)]
pub struct VendorShape {
    /// 厂商标识（F01/F02/F04/NV）。
    pub vendor: u32,
    /// 动词集形状序列（三组 API 的形状码）。
    pub shapes: [u32; NVAPI_GROUP_COUNT],
    /// 承诺域分隔模式在册。
    pub promise_domain_separated: bool,
}

/// 同构对拍器（判据"与 F01/F02/F04 厂商适配层同构"）。
#[derive(Debug)]
pub struct IsomorphismChecker {
    /// 厂商画像表。
    pub vendors: Vec<VendorShape>,
    /// 同构违约事件数（0x7402）。
    pub broken_events: u32,
}

impl IsomorphismChecker {
    /// 新对拍器。
    pub const fn new() -> IsomorphismChecker {
        IsomorphismChecker {
            vendors: Vec::new(),
            broken_events: 0,
        }
    }

    /// 登记厂商画像。
    pub fn enroll(&mut self, v: VendorShape) {
        self.vendors.push(v);
    }

    /// 同构裁决：全厂商形状序列与承诺域模式逐一一致（NVIDIA 与 F01/F02/F04
    /// 同构=形状逐位相同+域分隔全 true）。
    pub fn verify(&mut self, nv_shapes: &[u32; NVAPI_GROUP_COUNT]) -> Result<(), u32> {
        let mut i = 0;
        while i < self.vendors.len() {
            let v = &self.vendors[i];
            let mut j = 0;
            while j < NVAPI_GROUP_COUNT {
                if v.shapes[j] != nv_shapes[j] {
                    self.broken_events += 1;
                    return Err(XC_ISOMORPHISM_BROKEN);
                }
                j += 1;
            }
            if !v.promise_domain_separated {
                self.broken_events += 1;
                return Err(XC_ISOMORPHISM_BROKEN);
            }
            i += 1;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、十年承诺（承诺域分隔）
// ---------------------------------------------------------------------------

/// 十年承诺账（判据"十年承诺"——承诺域分隔防混签）。
#[derive(Debug)]
pub struct DecadePromise {
    /// 承诺域。
    pub domain: u32,
    /// 承诺文本指纹（承诺正文 FNV）。
    pub text_fp: u64,
    /// 总指纹（域+文本合成——总指纹变=承诺变 0x7405）。
    pub total_fp: u64,
    /// 失守事件数。
    pub broken_events: u32,
}

impl DecadePromise {
    /// 缔结承诺（域分隔+文本指纹+总指纹合成）。
    pub const fn make(text_fp: u64) -> DecadePromise {
        // 总指纹=域码 4 字节+文本指纹 8 字节入 FNV（const 展开四步+八步）。
        DecadePromise {
            domain: PROMISE_DOMAIN_NV,
            text_fp,
            total_fp: promise_total_fp(PROMISE_DOMAIN_NV, text_fp),
            broken_events: 0,
        }
    }

    /// 守诺校验：重算总指纹一致。
    pub fn verify(&mut self) -> Result<(), u32> {
        if promise_total_fp(self.domain, self.text_fp) != self.total_fp {
            self.broken_events += 1;
            return Err(XC_DECADE_PROMISE_BROKEN);
        }
        // 域分隔校验：NV 域承诺不得误用他域码。
        if self.domain != PROMISE_DOMAIN_NV {
            self.broken_events += 1;
            return Err(XC_DECADE_PROMISE_BROKEN);
        }
        Ok(())
    }
}

/// 承诺总指纹合成（域+文本）。
pub const fn promise_total_fp(domain: u32, text_fp: u64) -> u64 {
    let mut h = NVAPI_FNV_SEED;
    h = fnv_step(h, (domain & 0xFF) as u8);
    h = fnv_step(h, ((domain >> 8) & 0xFF) as u8);
    h = fnv_step(h, ((domain >> 16) & 0xFF) as u8);
    h = fnv_step(h, ((domain >> 24) & 0xFF) as u8);
    let mut i = 0;
    while i < 8 {
        h = fnv_step(h, ((text_fp >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 四、总引擎 + 自检（冻结/同构/承诺/判据四组+段对账）
// ---------------------------------------------------------------------------

/// NVIDIA 适配层 API 总引擎。
#[derive(Debug)]
pub struct NvApiEngine {
    /// 适配族注册表。
    pub reg: NvApiRegistry,
    /// 同构对拍器。
    pub iso: IsomorphismChecker,
    /// 十年承诺。
    pub promise: Option<DecadePromise>,
}

impl NvApiEngine {
    /// 出厂。
    pub const fn new() -> NvApiEngine {
        NvApiEngine {
            reg: NvApiRegistry::new(),
            iso: IsomorphismChecker::new(),
            promise: None,
        }
    }

    /// 组一：冻结——三组 API 注册指纹冻结；篡改显性；重复拒；绕窗拒。
    pub fn selfcheck_freeze(&mut self) -> Result<(), u32> {
        let mut e = NvApiEngine::new();
        // 三组 API 注册。
        let f1 = e.reg.register(b"nv_probe", 8, shape::PROBE, shape::PROBE)?;
        let f2 = e.reg.register(b"nv_diff", 7, shape::DIFF, shape::DIFF)?;
        let f3 = e.reg.register(b"nv_media_tag", 12, shape::MEDIA_TAG, shape::MEDIA_TAG)?;
        if f1 == f2 || f2 == f3 {
            return Err(XC_NVAPI_SIG_TAMPER); // 不同动词不同形状→指纹必异
        }
        // 冻结校验过。
        e.reg.verify_frozen()?;
        // 篡改：改动词首字节→指纹失配显性。
        e.reg.sigs[0].verb[0] = b'X';
        if e.reg.verify_frozen() != Err(XC_NVAPI_SIG_TAMPER) {
            return Err(XC_NVAPI_SIG_TAMPER);
        }
        if e.reg.tamper_events != 1 {
            return Err(XC_NVAPI_SIG_TAMPER);
        }
        // 重复注册拒。
        let mut e2 = NvApiEngine::new();
        e2.reg.register(b"nv_probe", 8, shape::PROBE, shape::PROBE)?;
        if e2.reg.register(b"nv_probe", 8, shape::PROBE, shape::PROBE) != Err(XC_NVAPI_DUP) {
            return Err(XC_NVAPI_DUP);
        }
        // 绕版本窗追加拒。
        if e2.reg.open_window_append(b"nv_extra", 8, shape::DIFF, shape::DIFF, false)
            != Err(XC_NVAPI_APPEND_ILLEGAL)
        {
            return Err(XC_NVAPI_APPEND_ILLEGAL);
        }
        // 合法版本窗追加过。
        e2.reg.open_window_append(b"nv_extra", 8, shape::DIFF, shape::DIFF, true)?;
        if e2.reg.version != 2 || e2.reg.sigs.len() != 2 {
            return Err(XC_NVAPI_APPEND_ILLEGAL);
        }
        Ok(())
    }

    /// 组二：同构——F01/F02/F04 与 NVIDIA 形状逐位一致；形状漂移/域分隔缺
    /// 失即 0x7402。
    pub fn selfcheck_isomorphism(&mut self) -> Result<(), u32> {
        let mut e = NvApiEngine::new();
        let nv_shapes = [shape::PROBE, shape::DIFF, shape::MEDIA_TAG];
        // F01/F02/F04 全同构入册。
        e.iso.enroll(VendorShape { vendor: 0x4630_3031, shapes: nv_shapes, promise_domain_separated: true });
        e.iso.enroll(VendorShape { vendor: 0x4630_3032, shapes: nv_shapes, promise_domain_separated: true });
        e.iso.enroll(VendorShape { vendor: 0x4630_3034, shapes: nv_shapes, promise_domain_separated: true });
        e.iso.verify(&nv_shapes)?;
        // 形状漂移案例：F02 差异表形状异→0x7402。
        let mut e2 = NvApiEngine::new();
        e2.iso.enroll(VendorShape { vendor: 0x4630_3031, shapes: nv_shapes, promise_domain_separated: true });
        e2.iso.enroll(VendorShape {
            vendor: 0x4630_3032,
            shapes: [shape::PROBE, shape::DIFF + 1, shape::MEDIA_TAG],
            promise_domain_separated: true,
        });
        if e2.iso.verify(&nv_shapes) != Err(XC_ISOMORPHISM_BROKEN) {
            return Err(XC_ISOMORPHISM_BROKEN);
        }
        // 域分隔缺失案例→0x7402。
        let mut e3 = NvApiEngine::new();
        e3.iso.enroll(VendorShape { vendor: 0x4630_3034, shapes: nv_shapes, promise_domain_separated: false });
        if e3.iso.verify(&nv_shapes) != Err(XC_ISOMORPHISM_BROKEN) {
            return Err(XC_ISOMORPHISM_BROKEN);
        }
        if e3.iso.broken_events != 1 {
            return Err(XC_ISOMORPHISM_BROKEN);
        }
        Ok(())
    }

    /// 组三：承诺——域分隔缔结；总指纹守恒；域码误用失守。
    pub fn selfcheck_promise(&mut self) -> Result<(), u32> {
        let mut e = NvApiEngine::new();
        // 缔结：承诺文本指纹（"NVIDIA adapter family frozen for a decade" 的 FNV 代表值）。
        let text_fp = sig_fingerprint(b"decade", shape::PROBE, 0x3130_5952); // "10YR"
        e.promise = Some(DecadePromise::make(text_fp));
        e.promise.as_mut().ok_or(XC_DECADE_PROMISE_BROKEN)?.verify()?;
        // 域码误用（换成 CERT 域码）→失守。
        let mut bad = DecadePromise::make(text_fp);
        bad.domain = 0x4345_5254; // "CERT" 域——混签即失守
        if bad.verify() != Err(XC_DECADE_PROMISE_BROKEN) {
            return Err(XC_DECADE_PROMISE_BROKEN);
        }
        // 总指纹篡改→失守。
        let mut e2 = NvApiEngine::new();
        e2.promise = Some(DecadePromise::make(text_fp));
        e2.promise.as_mut().ok_or(XC_DECADE_PROMISE_BROKEN)?.total_fp ^= 1;
        if e2.promise.as_mut().ok_or(XC_DECADE_PROMISE_BROKEN)?.verify()
            != Err(XC_DECADE_PROMISE_BROKEN)
        {
            return Err(XC_DECADE_PROMISE_BROKEN);
        }
        Ok(())
    }

    /// 组四：判据——端到端：三组注册→同构对拍→承诺缔结全链绿。
    pub fn selfcheck_verdict(&mut self) -> Result<(), u32> {
        let mut e = NvApiEngine::new();
        let nv_shapes = [shape::PROBE, shape::DIFF, shape::MEDIA_TAG];
        e.reg.register(b"nv_probe", 8, shape::PROBE, shape::PROBE)?;
        e.reg.register(b"nv_diff", 7, shape::DIFF, shape::DIFF)?;
        e.reg.register(b"nv_media_tag", 12, shape::MEDIA_TAG, shape::MEDIA_TAG)?;
        e.reg.verify_frozen()?;
        e.iso.enroll(VendorShape { vendor: 0x4630_3031, shapes: nv_shapes, promise_domain_separated: true });
        e.iso.enroll(VendorShape { vendor: 0x4630_3032, shapes: nv_shapes, promise_domain_separated: true });
        e.iso.enroll(VendorShape { vendor: 0x4630_3034, shapes: nv_shapes, promise_domain_separated: true });
        e.iso.verify(&nv_shapes)?;
        let text_fp = sig_fingerprint(b"decade", shape::PROBE, 0x3130_5952);
        e.promise = Some(DecadePromise::make(text_fp));
        e.promise.as_mut().ok_or(XC_DECADE_PROMISE_BROKEN)?.verify()?;
        // 全链账面：3 签名 0 事件。
        if e.reg.sigs.len() != 3
            || e.reg.tamper_events != 0
            || e.reg.dup_events != 0
            || e.reg.illegal_append_events != 0
            || e.iso.broken_events != 0
        {
            return Err(XC_NVAPI_SIG_TAMPER);
        }
        Ok(())
    }

    /// 全量自检入口（四组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_freeze()?;
        self.selfcheck_isomorphism()?;
        self.selfcheck_promise()?;
        self.selfcheck_verdict()?;
        Self::segment_guard()?;
        Ok(())
    }

    /// 段对账：五码全落 0x7400..0x7500 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_NVAPI_SIG_TAMPER,
            XC_ISOMORPHISM_BROKEN,
            XC_NVAPI_DUP,
            XC_NVAPI_APPEND_ILLEGAL,
            XC_DECADE_PROMISE_BROKEN,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x7400 || codes[i] >= 0x7500 {
                return Err(XC_NVAPI_SIG_TAMPER);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_NVAPI_SIG_TAMPER);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
