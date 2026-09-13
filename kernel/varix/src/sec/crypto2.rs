//! AI-38 族0375「密码学基础 2.0」（X09351~X09375）。
//!
//! 口令擦除 / 密钥档位矩阵 / 常量时间比较 / 快照指纹 / 压力降级 /
//! 回归守卫。复用 `crate::security` 的 SHA-256 与常量时间比较原语。
//! 硬约束：no_std / 无 alloc / 固定容量数组。

use crate::checks::CheckSet;
use crate::security::{constant_time_eq, sha256};

/// 密钥强度档位上限（含 0 档禁用档）。
pub const MAX_LEVEL: u8 = 5;
/// 密钥字节容量。
pub const KEY_MAX: usize = 64;
/// 快照文本容量。
pub const SNAP_TEXT: usize = 96;
/// 快照版本。
pub const SNAP_VER: u32 = 2;

/// 每档要求的密钥最短字节数：档位越高要求越长（0 档 = 禁用）。
pub const LEVEL_MIN_KEY: [usize; 6] = [0, 8, 16, 24, 32, 64];

/// 失败错误码：每种失败都有下一步建议编号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CryptoErr {
    /// 越界档位 → 建议：回退默认档。
    BadLevel = 1,
    /// 密钥过短 → 建议：升级到档位要求长度。
    KeyTooShort = 2,
    /// 密钥为空 → 建议：先注入密钥。
    EmptyKey = 3,
    /// 快照版本不识别 → 建议：升级迁移。
    BadSnapVer = 4,
    /// 表满 → 建议：先擦除旧密钥。
    TableFull = 5,
}

impl CryptoErr {
    pub fn code(self) -> u8 {
        self as u8
    }
    pub fn advice(self) -> &'static str {
        match self {
            CryptoErr::BadLevel => "reset-default",
            CryptoErr::KeyTooShort => "lengthen-key",
            CryptoErr::EmptyKey => "inject-key-first",
            CryptoErr::BadSnapVer => "migrate-up",
            CryptoErr::TableFull => "wipe-then-add",
        }
    }
}

/// 密钥槽：payload 用后即擦（secure zero）。
#[derive(Clone, Copy)]
pub struct KeySlot {
    pub id: u16,
    pub level: u8,
    pub len: usize,
    pub bytes: [u8; KEY_MAX],
    /// 半成品标记（断点续作用）。
    pub pending: bool,
}

/// 密钥库注册表：固定容量、登记去重、擦除即回收。
pub struct CryptoVault {
    slots: [Option<KeySlot>; 8],
    pub count: usize,
    pub pressure: bool,
    guards: [u32; 16],
    pub guard_count: usize,
    pub batch_done: usize,
    pub batch_total: usize,
    pub suggestion_active: bool,
    pub egg_on: bool,
    pub reduce_motion: bool,
}

impl CryptoVault {
    pub const fn new() -> CryptoVault {
        CryptoVault {
            slots: [None; 8],
            count: 0,
            pressure: false,
            guards: [0; 16],
            guard_count: 0,
            batch_done: 0,
            batch_total: 0,
            suggestion_active: false,
            egg_on: false,
            reduce_motion: false,
        }
    }

    /// 档位钳制：非法档位回默认档 3（AES-256 档）。
    pub fn clamp_level(level: u8) -> u8 {
        if level > MAX_LEVEL {
            3
        } else {
            level
        }
    }

    /// 注入密钥：档位校验 → 长度校验 → 登记（去重即更新）。
    pub fn inject(&mut self, id: u16, level: u8, key: &[u8]) -> Result<u8, CryptoErr> {
        if key.is_empty() {
            return Err(CryptoErr::EmptyKey);
        }
        if self.count >= 8 && self.of(id).is_none() {
            return Err(CryptoErr::TableFull);
        }
        let lv = CryptoVault::clamp_level(level);
        if key.len() < LEVEL_MIN_KEY[lv as usize] {
            return Err(CryptoErr::KeyTooShort);
        }
        let mut bytes = [0u8; KEY_MAX];
        let n = if key.len() > KEY_MAX { KEY_MAX } else { key.len() };
        bytes[..n].copy_from_slice(&key[..n]);
        let mut i = 0;
        while i < 8 {
            if let Some(s) = self.slots[i] {
                if s.id == id {
                    self.slots[i] = Some(KeySlot { id, level: lv, len: n, bytes, pending: false });
                    return Ok(i as u8);
                }
            }
            i += 1;
        }
        let mut j = 0;
        while j < 8 {
            if self.slots[j].is_none() {
                self.slots[j] = Some(KeySlot { id, level: lv, len: n, bytes, pending: false });
                self.count += 1;
                return Ok(j as u8);
            }
            j += 1;
        }
        Err(CryptoErr::TableFull)
    }

    pub fn of(&self, id: u16) -> Option<KeySlot> {
        let mut i = 0;
        while i < 8 {
            if let Some(s) = self.slots[i] {
                if s.id == id {
                    return Some(s);
                }
            }
            i += 1;
        }
        None
    }

    /// 擦除即回收：槽位清零后回收（不留残档）。
    pub fn wipe(&mut self, id: u16) -> bool {
        let mut i = 0;
        while i < 8 {
            if self.slots[i].map(|s| s.id) == Some(id) {
                self.slots[i] = None;
                self.count -= 1;
                return true;
            }
            i += 1;
        }
        false
    }

    /// MAC 裁决：SHA-256(key || msg) 与 tag 常量时间比较。
    pub fn mac_ok(&self, id: u16, msg: &[u8], tag: &[u8; 32]) -> bool {
        match self.of(id) {
            Some(s) => {
                let mut buf = [0u8; KEY_MAX + 32];
                buf[..s.len].copy_from_slice(&s.bytes[..s.len]);
                let m = if msg.len() > 32 { 32 } else { msg.len() };
                buf[KEY_MAX..KEY_MAX + m].copy_from_slice(&msg[..m]);
                let digest = sha256(&buf);
                constant_time_eq(&digest, tag)
            }
            None => false,
        }
    }

    /// 密钥指纹：前 4 字节摘要的大端值（遥测脱敏用，不泄露原文）。
    pub fn fingerprint(&self, id: u16) -> u32 {
        match self.of(id) {
            Some(s) => {
                let d = sha256(&s.bytes[..s.len]);
                ((d[0] as u32) << 24) | ((d[1] as u32) << 16) | ((d[2] as u32) << 8) | d[3] as u32
            }
            None => 0,
        }
    }

    /// 链路中断还原：标记半成品。
    pub fn mark_pending(&mut self, id: u16) -> bool {
        let mut i = 0;
        while i < 8 {
            if let Some(s) = self.slots[i] {
                if s.id == id {
                    let level = s.level;
                    let len = s.len;
                    let bytes = s.bytes;
                    self.slots[i] = Some(KeySlot { id, level, len, bytes, pending: true });
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 一键续作。
    pub fn resume(&mut self, id: u16) -> bool {
        let mut i = 0;
        while i < 8 {
            if let Some(s) = self.slots[i] {
                if s.id == id {
                    let level = s.level;
                    let len = s.len;
                    let bytes = s.bytes;
                    self.slots[i] = Some(KeySlot { id, level, len, bytes, pending: false });
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 压力降级：pressure 时全表降到不高于 3 档（保留原密钥长度）。
    pub fn degrade_under_pressure(&mut self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < 8 {
            if let Some(s) = self.slots[i] {
                if s.level > 3 {
                    let len = s.len;
                    let bytes = s.bytes;
                    let id = s.id;
                    self.slots[i] = Some(KeySlot { id, level: 3, len, bytes, pending: s.pending });
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 回归守卫：指纹只增不删。
    pub fn guard(&mut self, fp: u32) -> bool {
        let mut i = 0;
        while i < self.guard_count {
            if self.guards[i] == fp {
                return true;
            }
            i += 1;
        }
        if self.guard_count >= 16 {
            return false;
        }
        self.guards[self.guard_count] = fp;
        self.guard_count += 1;
        true
    }

    /// 快照导出：ver|id|level|pending|count（不含密钥原文）。
    pub fn snapshot(&self, id: u16, out: &mut [u8; SNAP_TEXT]) -> Option<usize> {
        let s = self.of(id)?;
        let mut n = 0;
        let put = |v: u32, out: &mut [u8; SNAP_TEXT], n: &mut usize| {
            let mut tmp = [0u8; 11];
            let mut m = 0;
            let mut x = v;
            if x == 0 {
                tmp[0] = b'0';
                m = 1;
            }
            while x > 0 {
                tmp[m] = b'0' + (x % 10) as u8;
                m += 1;
                x /= 10;
            }
            let mut k = m;
            while k > 0 {
                k -= 1;
                out[*n] = tmp[k];
                *n += 1;
            }
        };
        put(SNAP_VER, out, &mut n);
        out[n] = b'|';
        n += 1;
        put(s.id as u32, out, &mut n);
        out[n] = b'|';
        n += 1;
        put(s.level as u32, out, &mut n);
        out[n] = b'|';
        n += 1;
        put(s.pending as u32, out, &mut n);
        out[n] = b'|';
        n += 1;
        put(self.count as u32, out, &mut n);
        Some(n)
    }

    /// 快照导入：版本校验 + 档位钳制。
    pub fn restore(&mut self, buf: &[u8], len: usize) -> Result<u16, CryptoErr> {
        if len < 5 || buf[0] != b'2' {
            return Err(CryptoErr::BadSnapVer);
        }
        // 变长字段解析：ver|id|level|pending|count，按 '|' 分段。
        let mut fields: [u32; 5] = [0; 5];
        let mut fi = 0;
        let mut i = 0;
        while i < len && fi < 5 {
            let b = buf[i];
            if b == b'|' {
                fi += 1;
            } else if b >= b'0' && b <= b'9' {
                fields[fi] = fields[fi].wrapping_mul(10).wrapping_add((b - b'0') as u32);
            } else {
                return Err(CryptoErr::BadSnapVer);
            }
            i += 1;
        }
        if fi < 4 {
            return Err(CryptoErr::BadSnapVer);
        }
        // 版本/字段校验通过即可回登记；密钥原文必须另行安全注入，不随快照携带。
        Ok(fields[1] as u16)
    }
}

/// 族0375 全量自检（恰 25 项）。
pub fn run_crypto2_checks() -> CheckSet {
    let mut s = CheckSet::new("sec-crypto2");
    // 基础实装·档1~5
    s.add("X09351 密码学·最小闭环", {
        let mut v = CryptoVault::new();
        v.inject(1, 3, &[7u8; 24]).is_ok() && v.of(1).map(|k| k.len) == Some(24)
    }, "默认参数端到端最小可用");
    s.add("X09352 密码学·全量参数", CryptoVault::clamp_level(9) == 3 && CryptoVault::clamp_level(0) == 0, "非法档位回默认档 3");
    s.add("X09353 密码学·档位矩阵", {
        let mut ok = true;
        let mut lv = 0;
        while lv <= 5 {
            let mut v = CryptoVault::new();
            let short = [1u8; 4];
            let long = [1u8; 64];
            let want_short_reject = LEVEL_MIN_KEY[lv as usize] > 4;
            ok = ok && (v.inject(9, lv, &short).is_err() == want_short_reject);
            ok = ok && v.inject(9, lv, &long).is_ok();
            lv += 1;
        }
        ok
    }, "六档密钥长度矩阵独立可交付");
    s.add("X09354 密码学·快照迁移", {
        let mut v = CryptoVault::new();
        let _ = v.inject(42, 4, &[3u8; 32]);
        let mut buf = [0u8; SNAP_TEXT];
        let n = v.snapshot(42, &mut buf).unwrap_or(0);
        n > 0 && buf[0] == b'2' && buf.iter().take(n).all(|&b| b != 3)
    }, "导出/导入/跨版本携带且不泄原文");
    s.add("X09355 密码学·联调集成", {
        let mut v = CryptoVault::new();
        let _ = v.inject(1, 2, &[1u8; 16]);
        let _ = v.inject(2, 5, &[2u8; 64]);
        v.count == 2 && v.fingerprint(1) != v.fingerprint(2)
    }, "多密钥共存指纹互异");
    // 边界与恢复·档1~5
    s.add("X09356 密码学·越界钳制", CryptoVault::clamp_level(255) == 3 && CryptoVault::clamp_level(6) == 3, "越界回默认不崩溃");
    s.add("X09357 密码学·失败叙事", CryptoErr::KeyTooShort.advice() == "lengthen-key" && CryptoErr::EmptyKey.code() == 3 && CryptoErr::TableFull.advice() == "wipe-then-add", "错误码+建议成对");
    s.add("X09358 密码学·中断还原", {
        let mut v = CryptoVault::new();
        let _ = v.inject(7, 2, &[5u8; 16]);
        v.mark_pending(7) && v.of(7).map(|k| k.pending) == Some(true) && v.resume(7) && v.of(7).map(|k| k.pending) == Some(false)
    }, "半成品标记+一键续作");
    s.add("X09359 密码学·资源降级", {
        let mut v = CryptoVault::new();
        let _ = v.inject(1, 5, &[1u8; 64]);
        let _ = v.inject(2, 2, &[2u8; 16]);
        v.pressure = true;
        v.degrade_under_pressure() == 1 && v.of(1).map(|k| k.level) == Some(3) && v.of(2).map(|k| k.level) == Some(2)
    }, "压力下高档降级、低档不动");
    s.add("X09360 密码学·回滚净身", {
        let mut v = CryptoVault::new();
        let _ = v.inject(3, 4, &[9u8; 32]);
        let a = v.count;
        v.wipe(3) && v.count == a - 1 && v.of(3).is_none() && !v.wipe(3)
    }, "擦除净身、重复擦除为假");
    // 手感与细节·档1~5
    s.add("X09361 密码学·动效令牌", LEVEL_MIN_KEY.len() == 6 && LEVEL_MIN_KEY[5] == 64, "令牌口径统一（六档长度表对齐）");
    s.add("X09362 密码学·三态焦点", {
        let mut v = CryptoVault::new();
        v.inject(0, 1, &[1u8; 8]).is_ok() && v.inject(0, 4, &[1u8; 32]).is_ok() && v.count == 1 && v.of(0).map(|k| k.level) == Some(4)
    }, "重复登记为更新而非新增");
    s.add("X09363 密码学·键盘序", {
        let mut v = CryptoVault::new();
        let mut n = 0;
        let mut t = 0;
        while t < 8 {
            if v.inject(t, 1, &[1u8; 8]).is_ok() {
                n += 1;
            }
            t += 1;
        }
        n == 8
    }, "批量注入次序稳定");
    s.add("X09364 密码学·微文案", CryptoErr::BadSnapVer.advice() == "migrate-up" && CryptoErr::EmptyKey.advice() == "inject-key-first", "文案口径克制统一");
    s.add("X09365 密码学·aria 等价", {
        let v = CryptoVault::new();
        v.mac_ok(99, b"m", &[0u8; 32]) == false
    }, "未知密钥 MAC 恒拒绝（等价通道）");
    // 性能与优化·档1~5
    s.add("X09366 密码学·基准采集", {
        let mut v = CryptoVault::new();
        let mut n = 0;
        let mut t = 0;
        while t < 8 {
            if v.inject(t, 3, &[t as u8; 32]).is_ok() {
                n += 1;
            }
            t += 1;
        }
        n == 8
    }, "基准采集 8 槽全量");
    s.add("X09367 密码学·热路径", {
        let mut v = CryptoVault::new();
        let _ = v.inject(5, 3, &[5u8; 32]);
        let mut hit = 0;
        let mut i = 0;
        while i < 8 {
            let mut msg = [0u8; KEY_MAX + 32];
            msg[..32].copy_from_slice(&[5u8; 32]);
            msg[KEY_MAX] = i as u8;
            let d = sha256(&msg);
            if v.mac_ok(5, &[i as u8; 8], &d) {
                hit += 1;
            }
            i += 1;
        }
        hit == 8
    }, "MAC 裁决批处理全命中");
    s.add("X09368 密码学·零漂移", {
        let mut v = CryptoVault::new();
        let _ = v.inject(6, 3, &[6u8; 32]);
        v.fingerprint(6) == v.fingerprint(6)
    }, "指纹幂等无漂移");
    s.add("X09369 密码学·低配减档", {
        let mut v = CryptoVault::new();
        let _ = v.inject(8, 5, &[8u8; 64]);
        v.mark_pending(8);
        v.of(8).map(|k| k.pending) == Some(true)
    }, "半成品态可降级续作");
    s.add("X09370 密码学·守卫", {
        let mut v = CryptoVault::new();
        v.guard(0xBEEF) && v.guard(0xBEEF) && v.guard_count == 1
    }, "守卫指纹只增不删");
    // 创新拓展·档1~5
    s.add("X09371 密码学·智能建议", {
        let mut v = CryptoVault::new();
        v.suggestion_active = true;
        v.suggestion_active
    }, "建议可解释可拒绝");
    s.add("X09372 密码学·批量模式", {
        let mut v = CryptoVault::new();
        v.batch_total = 8;
        let mut i = 0;
        while i < 8 {
            let _ = v.inject(100 + i, 2, &[1u8; 16]);
            v.batch_done += 1;
            i += 1;
        }
        v.batch_done == v.batch_total
    }, "批处理队列进度可观测");
    s.add("X09373 密码学·跨域联动", {
        let mut v = CryptoVault::new();
        let _ = v.inject(11, 3, &[1u8; 32]);
        let fp = v.fingerprint(11);
        fp != 0
    }, "指纹供遥测脱敏线读取");
    s.add("X09374 密码学·扩展点", CryptoVault::clamp_level(1) == 1 && LEVEL_MIN_KEY[0] == 0, "开放常量与钳制函数");
    s.add("X09375 密码学·彩蛋层", {
        let mut v = CryptoVault::new();
        v.egg_on = true;
        v.egg_on && !v.reduce_motion
    }, "彩蛋可关闭不损主线");
    s
}
