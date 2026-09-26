
// ---------------------------------------------------------------------------
// F019 · 深化批次七：GUID 混合端字节序（COM CLSID 的 wire format）
//
// 主册依据（G-A-19【数据与存储】）：「COM 对象生命周期随进程」的序列化前提
// ——CLSID 的二进制形态是**混合端**：Data1(u32)/Data2(u16)/Data3(u16) 小端，
// Data4(8 字节) 原样连续（Windows RPC 规范钉值——注册表蜂巢序列化的字节序
// 唯一源）。
// ---------------------------------------------------------------------------

/// GUID → 16 字节 wire format（混合端）。
pub fn guid_to_bytes(d1: u32, d2: u16, d3: u16, d4: [u8; 8]) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&d1.to_le_bytes());
    out[4..6].copy_from_slice(&d2.to_le_bytes());
    out[6..8].copy_from_slice(&d3.to_le_bytes());
    out[8..16].copy_from_slice(&d4);
    out
}

/// 16 字节 wire format → GUID（逆向）。
pub fn guid_from_bytes(b: &[u8; 16]) -> (u32, u16, u16, [u8; 8]) {
    let d1 = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let d2 = u16::from_le_bytes([b[4], b[5]]);
    let d3 = u16::from_le_bytes([b[6], b[7]]);
    let mut d4 = [0u8; 8];
    d4.copy_from_slice(&b[8..16]);
    (d1, d2, d3, d4)
}

/// F019 深化批次七自检。
pub fn run_comloc_deep6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F019-comloc-deep6");
    // 1) ShellLink CLSID 锚（00021401-0000-0000-C000-000000000046）→ 已知
    //    wire bytes：01 14 02 00 00 00 00 00 C0 00 00 00 00 00 00 46。
    let bytes = guid_to_bytes(0x0002_1401, 0x0000, 0x0000, [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46]);
    cs.add(
        "guid_mixed_endian_shell_link_anchor",
        bytes
            == [0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
        "",
    );
    // 2) 双向 round-trip：任意 GUID 字节形态往返恒等。
    let (d1, d2, d3, d4) = guid_from_bytes(&bytes);
    cs.add(
        "guid_roundtrip",
        d1 == 0x0002_1401 && d2 == 0 && d3 == 0 && d4 == [0xC0, 0, 0, 0, 0, 0, 0, 0x46]
            && guid_from_bytes(&guid_to_bytes(0xDEAD_BEEF, 0x1234, 0x5678, [1, 2, 3, 4, 5, 6, 7, 8]))
                == (0xDEAD_BEEF, 0x1234, 0x5678, [1, 2, 3, 4, 5, 6, 7, 8]),
        "",
    );
    cs
}
