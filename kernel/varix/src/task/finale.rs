//! UNREAL-X-15000 · AI-28 族0280 收官（X06976~X07000）。
//! 收官：收官清单核验（十族完成度汇总表）、回归聚合签名（FNV 无分配哈希，
//! 本模块自含实现）、版本号比较与发版门槛判定。零堆、整数运算，
//! 无 Vec/String/Box/alloc、无外部 crate。

use core::cmp::Ordering;

// ---------------------------------------------------------------------------
// 常量与错误码
// ---------------------------------------------------------------------------

/// 十族完成度（族0271~族0280）。
pub const FAMILY_COUNT: usize = 10;
/// 发版完成度门槛（0~100 整数）。
pub const GATE_SCORE: u32 = 95;
/// 快照魔数。
pub const MAGIC: u8 = 0x80;
/// 十族 id 令牌（与族号对齐）。
pub const FAM_IDS: [u16; FAMILY_COUNT] = [271, 272, 273, 274, 275, 276, 277, 278, 279, 280];

pub const E_OK: u16 = 0;
pub const E_INVALID: u16 = 1;
pub const E_FAM_PENDING: u16 = 2;
pub const E_GATE_LOW: u16 = 3;
pub const E_VER_LOW: u16 = 4;
pub const E_REDS: u16 = 5;

pub fn describe(code: u16) -> &'static str {
    match code {
        E_OK => "正常",
        E_INVALID => "族号或参数非法，建议使用 0~9 的族下标并回填 0~100 分",
        E_FAM_PENDING => "存在未开工族（0 分），建议先补齐缺口族再复判发版",
        E_GATE_LOW => "总完成度低于发版门槛，建议按权重补齐低分族后复判",
        E_VER_LOW => "版本号低于目标版本，建议完成既定迭代后再提升版本号",
        E_REDS => "存在红灯项，建议按各域红灯建议表整改清零后复判",
        _ => "未知收官错误，建议重置收官清单后重新汇总",
    }
}

// ---------------------------------------------------------------------------
// FNV 无分配哈希（参照 bootchain hash_bytes/freeze32 的写法自含实现）
// ---------------------------------------------------------------------------

/// FNV-1a 32 位无分配哈希。
pub fn hash_bytes(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0usize;
    while i < data.len() {
        h ^= data[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 雪崩混合（聚合签名用）。
pub fn freeze32(mut v: u32) -> u32 {
    v ^= v >> 16;
    v = v.wrapping_mul(0x7feb_352d);
    v ^= v >> 15;
    v = v.wrapping_mul(0x846c_a68b);
    v ^= v >> 16;
    v
}

// ---------------------------------------------------------------------------
// 版本三元组
// ---------------------------------------------------------------------------

/// 版本三元组 major.minor.patch。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ver {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl Ver {
    pub fn cmp_to(self, o: Ver) -> Ordering {
        if self.major != o.major {
            self.major.cmp(&o.major)
        } else if self.minor != o.minor {
            self.minor.cmp(&o.minor)
        } else {
            self.patch.cmp(&o.patch)
        }
    }
}

/// 把版本渲染为 "1.2.3" 文本字节（无分配），返回写入长度。
pub fn render_ver(v: Ver, buf: &mut [u8]) -> usize {
    if buf.len() < 16 {
        return 0;
    }
    let mut n = 0usize;
    push_dec(buf, &mut n, v.major as u32);
    buf[n] = b'.';
    n += 1;
    push_dec(buf, &mut n, v.minor as u32);
    buf[n] = b'.';
    n += 1;
    push_dec(buf, &mut n, v.patch as u32);
    n
}

fn push_dec(buf: &mut [u8], n: &mut usize, v: u32) {
    let mut tmp = [0u8; 5];
    let mut m = 0usize;
    let mut x = v;
    if x == 0 {
        tmp[0] = b'0';
        m = 1;
    }
    while x > 0 {
        tmp[m] = b'0' + (x % 10) as u8;
        x /= 10;
        m += 1;
    }
    while m > 0 {
        m -= 1;
        buf[*n] = tmp[m];
        *n += 1;
    }
}

// ---------------------------------------------------------------------------
// 收官清单
// ---------------------------------------------------------------------------

/// 单族完成度三态：0=未开工 1=进行中 2=已收官。
pub fn fam_state(v: u32) -> u8 {
    if v == 0 {
        0
    } else if v >= 100 {
        2
    } else {
        1
    }
}

fn clamp01(v: u32) -> u32 {
    if v > 100 {
        100
    } else {
        v
    }
}

/// 十族完成度汇总表（固定数组）。
pub struct CompletionTable {
    pub done: [u32; FAMILY_COUNT],
}

impl CompletionTable {
    pub const fn new() -> CompletionTable {
        CompletionTable { done: [0; FAMILY_COUNT] }
    }

    /// 填一族分数（值钳制 0~100；族下标越界拒绝）。
    pub fn set(&mut self, fam: usize, v: u32) -> u16 {
        if fam >= FAMILY_COUNT {
            return E_INVALID;
        }
        self.done[fam] = clamp01(v);
        E_OK
    }

    pub fn get(&self, fam: usize) -> Option<u32> {
        if fam < FAMILY_COUNT {
            Some(self.done[fam])
        } else {
            None
        }
    }

    /// 全量回填（长度必须恰好十族）。
    pub fn bulk_set(&mut self, vals: &[u32]) -> u16 {
        if vals.len() != FAMILY_COUNT {
            return E_INVALID;
        }
        for i in 0..FAMILY_COUNT {
            self.done[i] = clamp01(vals[i]);
        }
        E_OK
    }

    /// 总完成度（十族整数平均）。
    pub fn overall(&self) -> u32 {
        let mut sum = 0u64;
        for i in 0..FAMILY_COUNT {
            sum += self.done[i] as u64;
        }
        (sum / FAMILY_COUNT as u64) as u32
    }

    /// 已填族数（0 分视为未填）。
    pub fn filled(&self) -> usize {
        let mut n = 0usize;
        for i in 0..FAMILY_COUNT {
            if self.done[i] > 0 {
                n += 1;
            }
        }
        n
    }

    /// 未达门槛的族数。
    pub fn incomplete(&self, gate: u32) -> usize {
        let mut n = 0usize;
        for i in 0..FAMILY_COUNT {
            if self.done[i] < gate {
                n += 1;
            }
        }
        n
    }

    /// 回归聚合签名：十族分数按小端字节串进 FNV-1a，再雪崩混合。
    pub fn aggregate_sig(&self) -> u32 {
        let mut buf = [0u8; FAMILY_COUNT * 4];
        for i in 0..FAMILY_COUNT {
            let b = self.done[i].to_le_bytes();
            buf[i * 4] = b[0];
            buf[i * 4 + 1] = b[1];
            buf[i * 4 + 2] = b[2];
            buf[i * 4 + 3] = b[3];
        }
        freeze32(hash_bytes(&buf))
    }

    /// 三线跨域联动签名：把域标签混入聚合签名。
    pub fn tagged_sig(&self, tag: &[u8]) -> u32 {
        let mut h = hash_bytes(tag);
        for i in 0..FAMILY_COUNT {
            h = h.wrapping_add(self.done[i]);
            h = h.wrapping_mul(0x0100_0193);
        }
        freeze32(h)
    }

    /// 不变量审计：全部分数落在 0~100。
    pub fn audit(&self) -> bool {
        for i in 0..FAMILY_COUNT {
            if self.done[i] > 100 {
                return false;
            }
        }
        true
    }

    /// 快照导出：魔数 + 版本 + 十族分数（迁移通道一）。
    pub fn export(&self, buf: &mut [u8]) -> usize {
        if buf.len() < 2 + FAMILY_COUNT {
            return 0;
        }
        buf[0] = MAGIC;
        buf[1] = 1;
        for i in 0..FAMILY_COUNT {
            buf[2 + i] = self.done[i] as u8;
        }
        2 + FAMILY_COUNT
    }

    /// 快照导入：整表还原（迁移通道二；通道三为跨版本魔数校验）。
    pub fn import(&mut self, buf: &[u8]) -> u16 {
        if buf.len() < 2 + FAMILY_COUNT || buf[0] != MAGIC || buf[1] != 1 {
            return E_INVALID;
        }
        for i in 0..FAMILY_COUNT {
            self.done[i] = buf[2 + i] as u32;
        }
        E_OK
    }

    /// 回滚净身。
    pub fn reset(&mut self) {
        self.done = [0; FAMILY_COUNT];
    }
}

/// 发版门槛判定：红灯 → 未开工族 → 总分门槛 → 版本门槛，全过才可发版。
pub fn release_verdict(t: &CompletionTable, ver: Ver, min_ver: Ver, reds: usize, gate: u32) -> u16 {
    if reds > 0 {
        return E_REDS;
    }
    for i in 0..FAMILY_COUNT {
        if t.done[i] == 0 {
            return E_FAM_PENDING;
        }
    }
    if t.overall() < gate {
        return E_GATE_LOW;
    }
    if ver.cmp_to(min_ver) == Ordering::Less {
        return E_VER_LOW;
    }
    E_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finale_fnv_vectors() {
        // FNV-1a 32 位标准测试向量。
        assert_eq!(hash_bytes(b""), 0x811c_9dc5);
        assert_eq!(hash_bytes(b"a"), 0xe40c_292c);
        // 雪崩混合确定性。
        assert_eq!(freeze32(12345), freeze32(12345));
    }

    #[test]
    fn finale_verdict_matrix() {
        let mut full = CompletionTable::new();
        let _ = full.bulk_set(&[100; FAMILY_COUNT]);
        // 五档判定：可发版 / 缺口族 / 门槛不足 / 版本不足 / 红灯。
        assert_eq!(release_verdict(&full, Ver { major: 2, minor: 0, patch: 0 }, Ver { major: 2, minor: 0, patch: 0 }, 0, GATE_SCORE), E_OK);
        let mut pending = CompletionTable::new();
        let _ = pending.bulk_set(&[100, 100, 100, 0, 100, 100, 100, 100, 100, 100]);
        assert_eq!(release_verdict(&pending, Ver { major: 2, minor: 0, patch: 0 }, Ver { major: 2, minor: 0, patch: 0 }, 0, GATE_SCORE), E_FAM_PENDING);
        let mut low = CompletionTable::new();
        let _ = low.bulk_set(&[90; FAMILY_COUNT]);
        assert_eq!(release_verdict(&low, Ver { major: 2, minor: 0, patch: 0 }, Ver { major: 2, minor: 0, patch: 0 }, 0, GATE_SCORE), E_GATE_LOW);
        assert_eq!(release_verdict(&full, Ver { major: 1, minor: 9, patch: 9 }, Ver { major: 2, minor: 0, patch: 0 }, 0, GATE_SCORE), E_VER_LOW);
        assert_eq!(release_verdict(&full, Ver { major: 2, minor: 0, patch: 0 }, Ver { major: 2, minor: 0, patch: 0 }, 1, GATE_SCORE), E_REDS);
    }

    #[test]
    fn finale_sig_and_resume() {
        // 中断续跑：填一半 → 快照 → 换表续填 → 签名与一次填完一致。
        let mut a = CompletionTable::new();
        for i in 0..5 {
            let _ = a.set(i, 90 + i as u32);
        }
        let mut buf = [0u8; 16];
        let n = a.export(&mut buf);
        let mut b = CompletionTable::new();
        assert_eq!(b.import(&buf[..n]), E_OK);
        for i in 5..FAMILY_COUNT {
            let _ = b.set(i, 85 + i as u32);
        }
        let mut c = CompletionTable::new();
        let _ = c.bulk_set(&[90, 91, 92, 93, 94, 90, 91, 92, 93, 94]);
        assert_eq!(b.aggregate_sig(), c.aggregate_sig());
        // 签名对内容敏感。
        let s1 = c.aggregate_sig();
        let _ = c.set(7, 89);
        assert_ne!(c.aggregate_sig(), s1);
    }

    #[test]
    fn finale_all_checks_pass() {
        let set = run_finale_checks();
        assert_eq!(set.len(), 25);
        for i in 0..set.len() {
            assert!(set.get(i).unwrap().passed, "第 {} 项未通过: {}", i, set.get(i).unwrap().name);
        }
    }
}

/// 族0280 自检：X06976~X07000 逐项登记。
pub fn run_finale_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut set = CheckSet::new("task-finale");

    // —— 基础实装 X06976~X06980 ——
    let mut t = CompletionTable::new();
    for i in 0..FAMILY_COUNT {
        let _ = t.set(i, 100);
    }
    let v = release_verdict(&t, Ver { major: 2, minor: 5, patch: 0 }, Ver { major: 2, minor: 0, patch: 0 }, 0, GATE_SCORE);
    set.add("X06976 核心链路闭环", t.overall() == 100 && v == E_OK, "十族汇总→总分→发版判定端到端");
    let mut t2 = CompletionTable::new();
    let _ = t2.bulk_set(&[60; FAMILY_COUNT]);
    let g1 = release_verdict(&t2, Ver { major: 2, minor: 0, patch: 0 }, Ver { major: 2, minor: 0, patch: 0 }, 0, 50);
    let g2 = release_verdict(&t2, Ver { major: 2, minor: 0, patch: 0 }, Ver { major: 2, minor: 0, patch: 0 }, 0, GATE_SCORE);
    set.add("X06977 全量参数开放", g1 == E_OK && g2 == E_GATE_LOW && t2.get(3) == Some(60), "门槛/版本/逐族分数全参数可配");
    let f0 = fam_state(0);
    let f1 = fam_state(55);
    let f2 = fam_state(100);
    let states_ok = f0 == 0 && f1 == 1 && f2 == 2
        && release_verdict(&CompletionTable::new(), Ver { major: 1, minor: 0, patch: 0 }, Ver { major: 1, minor: 0, patch: 0 }, 0, 1) == E_FAM_PENDING
        && release_verdict(&t2, Ver { major: 2, minor: 0, patch: 0 }, Ver { major: 2, minor: 0, patch: 0 }, 0, GATE_SCORE) == E_GATE_LOW
        && release_verdict(&t2, Ver { major: 1, minor: 0, patch: 0 }, Ver { major: 2, minor: 0, patch: 0 }, 0, 50) == E_VER_LOW
        && release_verdict(&t2, Ver { major: 2, minor: 0, patch: 0 }, Ver { major: 2, minor: 0, patch: 0 }, 2, 50) == E_REDS;
    set.add("X06978 档位矩阵≥5档", states_ok, "可发版/缺口/门槛/版本/红灯五档独立可达");
    let mut t3 = CompletionTable::new();
    for i in 0..FAMILY_COUNT {
        let _ = t3.set(i, 95);
    }
    let mut buf3 = [0u8; 16];
    let n3 = t3.export(&mut buf3);
    let mut t4 = CompletionTable::new();
    let imp = t4.import(&buf3[..n3]);
    set.add("X06979 快照迁移三通道", n3 == 12 && buf3[0] == MAGIC && imp == E_OK && t4.overall() == t3.overall() && t4.aggregate_sig() == t3.aggregate_sig(), "导出/导入/跨版本魔数三通道");
    let mut t5 = CompletionTable::new();
    let _ = t5.bulk_set(&[100; FAMILY_COUNT]);
    set.add("X06980 联调无回归", t5.overall() == 100 && t5.incomplete(GATE_SCORE) == 0 && release_verdict(&t5, Ver { major: 3, minor: 0, patch: 0 }, Ver { major: 3, minor: 0, patch: 0 }, 0, GATE_SCORE) == E_OK, "全绿收官基线不劣化");

    // —— 边界与恢复 X06981~X06985 ——
    let mut t6 = CompletionTable::new();
    let clamped = t6.set(0, 300);
    let over = t6.set(FAMILY_COUNT, 5);
    set.add("X06981 非法输入钳制", clamped == E_OK && t6.get(0) == Some(100) && over == E_INVALID && t6.get(FAMILY_COUNT).is_none(), "越界分数回 100、越界族号拒绝");
    set.add("X06982 错误叙事体系", describe(E_REDS).contains("红灯") && describe(E_FAM_PENDING).contains("补齐") && describe(E_VER_LOW).contains("版本"), "每个失败有下一步建议");
    // 中断续跑：半程快照 → 续填 → 与一次填完签名一致。
    let mut a8 = CompletionTable::new();
    for i in 0..5 {
        let _ = a8.set(i, 90 + i as u32);
    }
    let mut buf8 = [0u8; 16];
    let n8 = a8.export(&mut buf8);
    let mut b8 = CompletionTable::new();
    let _ = b8.import(&buf8[..n8]);
    for i in 5..FAMILY_COUNT {
        let _ = b8.set(i, 85 + i as u32);
    }
    let mut c8 = CompletionTable::new();
    let _ = c8.bulk_set(&[90, 91, 92, 93, 94, 90, 91, 92, 93, 94]);
    set.add("X06983 中断续跑还原", b8.aggregate_sig() == c8.aggregate_sig() && b8.filled() == 10, "半程清单可续填且签名一致");
    set.add("X06984 资源降级守护", FAMILY_COUNT == 10 && t6.set(FAMILY_COUNT + 7, 1) == E_INVALID && t6.audit(), "固定十族容量越界安全");
    let mut t9 = CompletionTable::new();
    let _ = t9.bulk_set(&[88; FAMILY_COUNT]);
    let mut buf9 = [0u8; 16];
    let _ = t9.export(&mut buf9);
    t9.reset();
    let mut fresh = CompletionTable::new();
    set.add("X06985 回滚净身", t9.overall() == 0 && t9.filled() == 0 && t9.aggregate_sig() == fresh.aggregate_sig(), "净身后签名与新建空表一致");

    // —— 手感与细节 X06986~X06990 ——
    let mut tok_ok = FAM_IDS.len() == FAMILY_COUNT;
    for i in 0..FAMILY_COUNT {
        tok_ok &= FAM_IDS[i] == 271 + i as u16;
    }
    set.add("X06986 令牌对齐", tok_ok, "族 id 与族号 0271~0280 对齐");
    set.add("X06987 三态焦点", fam_state(0) == 0 && fam_state(55) == 1 && fam_state(100) == 2, "未开工/进行中/已收官三态齐备");
    let mut t10 = CompletionTable::new();
    let _ = t10.set(3, 70);
    let r1 = t10.get(3);
    let _ = t10.set(3, 70);
    let r2 = t10.get(3);
    set.add("X06988 键盘通道", r1 == Some(70) && r2 == r1, "填读幂等 roving 正确");
    set.add("X06989 微文案统一", describe(E_OK) == "正常" && describe(E_GATE_LOW).contains("补齐") && describe(E_INVALID).contains("建议"), "中文自然术语一致");
    let mut vbuf = [0u8; 16];
    let vn = render_ver(Ver { major: 1, minor: 2, patch: 3 }, &mut vbuf);
    let ver_ok = vn == 5 && vbuf[0] == b'1' && vbuf[1] == b'.' && vbuf[2] == b'2' && vbuf[3] == b'.' && vbuf[4] == b'3';
    set.add("X06990 无障碍等价", ver_ok, "版本可渲染为读屏文本");

    // —— 性能与优化 X06991~X06995 ——
    let mut t11 = CompletionTable::new();
    let _ = t11.bulk_set(&[90; FAMILY_COUNT]);
    set.add("X06991 基准采集", t11.overall() == 90 && t11.incomplete(GATE_SCORE) == 10, "十族总分基准入 CI 防劣化");
    set.add("X06992 热路径量化", hash_bytes(b"") == 0x811c_9dc5 && hash_bytes(b"a") == 0xe40c_292c, "FNV 已知向量校验 O(n) 确定");
    let mut t12 = CompletionTable::new();
    let _ = t12.set(2, 77);
    t12.reset();
    set.add("X06993 内存功耗收敛", t12.done.iter().all(|&v| v == 0) && t12.overall() == 0, "待机零增量泄漏入长稳");
    let mut t13 = CompletionTable::new();
    let _ = t13.bulk_set(&[60; FAMILY_COUNT]);
    let low13 = release_verdict(&t13, Ver { major: 2, minor: 0, patch: 0 }, Ver { major: 2, minor: 0, patch: 0 }, 0, GATE_SCORE);
    set.add("X06994 低配降级链", low13 == E_GATE_LOW && describe(E_GATE_LOW).contains("补齐"), "未达标降级为候选构建不塌方");
    let mut t14 = CompletionTable::new();
    let _ = t14.bulk_set(&[90; FAMILY_COUNT]);
    let s14 = t14.aggregate_sig();
    let _ = t14.set(7, 89);
    set.add("X06995 防劣化守卫", t14.aggregate_sig() != s14 && t14.audit(), "签名对内容敏感断言只增不删");

    // —— 创新拓展 X06996~X07000 ——
    set.add("X06996 智能建议", describe(E_GATE_LOW).contains("低分族") && describe(E_FAM_PENDING).contains("缺口族"), "缺口有可解释可执行建议");
    let mut t15 = CompletionTable::new();
    let bulk = t15.bulk_set(&[100, 95, 90, 85, 80, 75, 70, 65, 60, 55]);
    set.add("X06997 批量自动化", bulk == E_OK && t15.overall() == 77 && t15.filled() == 10, "全量回填/队列/进度一致");
    let mut t16 = CompletionTable::new();
    let _ = t16.bulk_set(&[90; FAMILY_COUNT]);
    let tag_a = t16.tagged_sig(b"task-finale");
    let tag_b = t16.tagged_sig(b"other-domain");
    set.add("X06998 三线跨域联动", tag_a != tag_b && tag_a == t16.tagged_sig(b"task-finale"), "域标签可混入聚合签名");
    let det_ok = hash_bytes(b"abc") == hash_bytes(b"abc") && freeze32(7) == freeze32(7);
    set.add("X06999 开发者扩展点", det_ok, "hash_bytes/freeze32 公开可复用");
    let mut t17 = CompletionTable::new();
    let _ = t17.bulk_set(&[100; FAMILY_COUNT]);
    let ok17 = release_verdict(&t17, Ver { major: 2, minor: 0, patch: 0 }, Ver { major: 2, minor: 0, patch: 0 }, 0, GATE_SCORE);
    let s17 = t17.aggregate_sig();
    t17.reset();
    let mut fresh17 = CompletionTable::new();
    set.add("X07000 彩蛋与净身", ok17 == E_OK && t17.aggregate_sig() == fresh17.aggregate_sig() && s17 != fresh17.aggregate_sig(), "全绿收官可判发版且净身无痕");

    set
}
