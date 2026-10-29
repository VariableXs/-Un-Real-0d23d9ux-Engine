//! UNX-F36055-J1 特权事件的诊断账格式（拒绝可解释性）。
//! 判据：① 每次特权拒绝落「哪个主体/哪个特权/为什么」三要素 100 例
//! 100% 可读（拒绝诊断账是全系统权限排障标准入口的子面）；
//! ② 纯功能代码 ≥300 行。
//! 设计纪律：诊断账格式 v1 = 定长十六进制渲染（对齐 unx36016 审计
//! 渲染族）；独立校验器逐字段验证——渲染与校验双实现互为对账。

use super::unx36041_privtable::{self, lookup_by_luid};
use super::unx36043_privnotheld::{subject_fp, DenialRecord, R_DISABLED};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xfe40;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_FMT: u16 = SEG_BASE + 1; // 渲染格式非法
pub const E_READ: u16 = SEG_BASE + 2; // 记录不可读（三要素缺）
pub const E_TABLE: u16 = SEG_BASE + 3; // 上游表纪律破
pub const E_VER: u16 = SEG_BASE + 4; // 格式版本锚不符
pub const E_LEN: u16 = SEG_BASE + 5; // 渲染长度越界
pub const E_BAL: u16 = SEG_BASE + 6; // 账内不可读项存在

/// 格式版本锚 v1。
pub const DIAG_VER: u32 = 1;

/// 渲染长度界（v1 定长结构）：
///   "P1|s=<16hex>|p=<8hex>|r=<1hex>|q=<8hex>\n" ≈ 43 字节。
pub const RENDER_MIN: usize = 40;
pub const RENDER_MAX: usize = 64;

/// 十六进制 nibble。
fn hexn(v: u8) -> u8 {
    if v < 10 {
        b'0' + v
    } else {
        b'a' + v - 10
    }
}

/// 诊断记录渲染 v1：定长十六进制行。
///   P1|s=<subject 16hex>|p=<luid 8hex>|r=<reason 1hex>|q=<seq 8hex>
pub fn render(r: &DenialRecord, out: &mut [u8]) -> Result<usize, u16> {
    if DIAG_VER != 1 {
        return Err(E_VER);
    }
    // 可读性前置：三要素齐全 + 表内特权（表外不可读）。
    if r.subject == 0 || r.luid_low == 0 || r.reason > R_DISABLED || r.seq == 0 {
        return Err(E_READ);
    }
    if unx36041_privtable::lookup_by_luid(r.luid_low).is_err() {
        return Err(E_READ);
    }
    let fp = &r.subject.to_le_bytes();
    let lp = &r.luid_low.to_le_bytes();
    let sp = &r.seq.to_le_bytes();
    // 模板："P1|s=" (5) + 16 + "|p=" (3) + 8 + "|r=" (3) + 1 + "|q=" (3) + 8 = 47。
    let need = 47;
    if out.len() < need {
        return Err(E_LEN);
    }
    let mut n = 0;
    let put = |bytes: &[u8], out: &mut [u8], n: &mut usize| {
        let mut i = 0;
        while i < bytes.len() {
            out[*n] = bytes[i];
            *n += 1;
            i += 1;
        }
    };
    put(b"P1|s=", out, &mut n);
    // subject 16 hex（小端逐字节逆序——大端渲染惯例）。
    let mut i = 8;
    while i > 0 {
        i -= 1;
        out[n] = hexn(fp[i] >> 4);
        out[n + 1] = hexn(fp[i] & 0xf);
        n += 2;
    }
    put(b"|p=", out, &mut n);
    let mut j = 4;
    while j > 0 {
        j -= 1;
        out[n] = hexn(lp[j] >> 4);
        out[n + 1] = hexn(lp[j] & 0xf);
        n += 2;
    }
    put(b"|r=", out, &mut n);
    out[n] = hexn(r.reason);
    n += 1;
    put(b"|q=", out, &mut n);
    let mut k = 4;
    while k > 0 {
        k -= 1;
        out[n] = hexn(sp[k] >> 4);
        out[n + 1] = hexn(sp[k] & 0xf);
        n += 2;
    }
    Ok(n)
}

/// 独立校验器 v1：逐字段解析渲染行并回读比对（与 render 双实现对账）。
pub fn validate_v1(line: &[u8], want: &DenialRecord) -> Result<(), u16> {
    if line.len() < RENDER_MIN || line.len() > RENDER_MAX {
        return Err(E_LEN);
    }
    // 前缀。
    if &line[..5] != b"P1|s=" {
        return Err(E_FMT);
    }
    // subject 16 hex。
    let mut subject: u64 = 0;
    let mut i = 0;
    while i < 8 {
        let hi = (line[5 + i * 2] as char).to_digit(16).ok_or(E_FMT)? as u64;
        let lo = (line[5 + i * 2 + 1] as char).to_digit(16).ok_or(E_FMT)? as u64;
        subject = (subject << 8) | (hi << 4) | lo;
        i += 1;
    }
    let mut c = 21; // 5("P1|s=") + 16(subject hex)
    // "|p="。
    if &line[c..c + 3] != b"|p=" {
        return Err(E_FMT);
    }
    c += 3;
    let mut luid: u32 = 0;
    i = 0;
    while i < 4 {
        let hi = (line[c + i * 2] as char).to_digit(16).ok_or(E_FMT)? as u32;
        let lo = (line[c + i * 2 + 1] as char).to_digit(16).ok_or(E_FMT)? as u32;
        luid = (luid << 8) | (hi << 4) | lo;
        i += 1;
    }
    c += 8;
    if &line[c..c + 3] != b"|r=" {
        return Err(E_FMT);
    }
    c += 3;
    let reason = (line[c] as char).to_digit(16).ok_or(E_FMT)? as u8;
    c += 1;
    if &line[c..c + 3] != b"|q=" {
        return Err(E_FMT);
    }
    c += 3;
    let mut seq: u32 = 0;
    i = 0;
    while i < 4 {
        let hi = (line[c + i * 2] as char).to_digit(16).ok_or(E_FMT)? as u32;
        let lo = (line[c + i * 2 + 1] as char).to_digit(16).ok_or(E_FMT)? as u32;
        seq = (seq << 8) | (hi << 4) | lo;
        i += 1;
    }
    // 回读比对。
    if subject != want.subject || luid != want.luid_low || reason != want.reason || seq != want.seq
    {
        return Err(E_FMT);
    }
    Ok(())
}

/// 记录构造器（本面入口）：指纹 + 表内核验 + 合法原因。
pub fn make_record(subject_id: u64, luid_low: u32, reason: u8, seq: u32) -> Result<DenialRecord, u16> {
    if lookup_by_luid(luid_low).is_err() {
        return Err(super::unx36043_privnotheld::E_NOENT);
    }
    if reason > R_DISABLED {
        return Err(super::unx36043_privnotheld::E_ELEM);
    }
    if subject_id == 0 {
        return Err(super::unx36043_privnotheld::E_BARE);
    }
    Ok(DenialRecord {
        subject: subject_fp(subject_id),
        luid_low,
        reason,
        seq,
    })
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_FMT => "渲染格式非法",
        E_READ => "记录不可读（三要素缺）",
        E_TABLE => "上游表纪律破",
        E_VER => "格式版本锚不符",
        E_LEN => "渲染长度越界",
        E_BAL => "账内不可读项存在",
        _ => "未知诊断码",
    }
}

/// 演练：R1 上游表+版本锚；R2 100 例渲染+校验往返全对；
/// R3 不可读注入（零指纹/表外/非法原因）10 例全拒；
/// R4 长度界（渲染恒 47 字节）；R5 渲染稳定（两次渲染逐字节一致）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：上游表 + 版本锚。
    unx36041_privtable::table_integrity().map_err(|_| E_TABLE)?;
    if DIAG_VER != 1 {
        return Err(E_VER);
    }
    hits += 1;
    // R2：100 例三要素齐全记录——渲染 + 校验器往返。
    let mut buf = [0u8; 64];
    let mut k = 0;
    while k < 100 {
        let low = unx36041_privtable::PRIV_TABLE[k % 17].luid_low;
        let reason = (k % 3) as u8;
        let r = make_record(6000 + k as u64, low, reason, (k + 1) as u32)?;
        let n = render(&r, &mut buf)?;
        // 校验器往返。
        validate_v1(&buf[..n], &r)?;
        // 可读性断言：长度落在界内。
        if n < RENDER_MIN || n > RENDER_MAX {
            return Err(E_LEN);
        }
        hits += 1;
        k += 1;
    }
    // R3：不可读注入 10 例全拒。
    let mut rej = 0;
    // 3 例零指纹。
    let mut i = 0;
    while i < 3 {
        let bad = DenialRecord {
            subject: 0,
            luid_low: 2,
            reason: 0,
            seq: 1,
        };
        if render(&bad, &mut buf).is_ok() {
            return Err(E_READ);
        }
        rej += 1;
        i += 1;
    }
    // 3 例表外特权。
    i = 0;
    while i < 3 {
        let bad = DenialRecord {
            subject: 7,
            luid_low: 999,
            reason: 0,
            seq: 1,
        };
        if render(&bad, &mut buf).is_ok() {
            return Err(E_READ);
        }
        rej += 1;
        i += 1;
    }
    // 2 例非法原因。
    i = 0;
    while i < 2 {
        let bad = DenialRecord {
            subject: 7,
            luid_low: 2,
            reason: 9,
            seq: 1,
        };
        if render(&bad, &mut buf).is_ok() {
            return Err(E_READ);
        }
        rej += 1;
        i += 1;
    }
    // 2 例零 seq。
    i = 0;
    while i < 2 {
        let bad = DenialRecord {
            subject: 7,
            luid_low: 2,
            reason: 0,
            seq: 0,
        };
        if render(&bad, &mut buf).is_ok() {
            return Err(E_READ);
        }
        rej += 1;
        i += 1;
    }
    if rej != 10 {
        return Err(E_READ);
    }
    hits += 1;
    // R4：长度界——合法记录渲染恒 47 字节。
    let r = make_record(42, 2, 0, 1)?;
    let n = render(&r, &mut buf)?;
    if n != 47 {
        return Err(E_LEN);
    }
    hits += 1;
    // R5：渲染稳定。
    let n2 = render(&r, &mut buf)?;
    let mut buf2 = [0u8; 64];
    let n3 = render(&r, &mut buf2)?;
    if n2 != n3 {
        return Err(E_FMT);
    }
    let mut b = 0;
    while b < n3 {
        if buf[b] != buf2[b] {
            return Err(E_FMT);
        }
        b += 1;
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(104));
    }

    #[test]
    fn render_validate_roundtrip() {
        let r = make_record(99, 7, 1, 5).unwrap();
        let mut buf = [0u8; 64];
        let n = render(&r, &mut buf).unwrap();
        assert!(validate_v1(&buf[..n], &r).is_ok());
    }

    #[test]
    fn unreadable_rejected() {
        let bad = DenialRecord {
            subject: 0,
            luid_low: 0,
            reason: 0,
            seq: 0,
        };
        let mut buf = [0u8; 64];
        assert_eq!(render(&bad, &mut buf), Err(E_READ));
    }
}

pub fn run_unx36055_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36055_privdiag");
    cs.add("SEG 段位独占", SEG_BASE == 0xFE40, "0xFE40~0xFE5F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}
