# -*- coding: utf-8 -*-
"""取证增强：dump 事件环全 16 槽 ctrl + cmd 环前 4 槽 ctrl。"""
from pathlib import Path

p = Path(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\kernel\varix\src\drivers\xhci.rs")
s = p.read_text(encoding="utf-8")

old = """                    crate::kwarn!(
                        "xhci: forensic evt_ptr={:#x} evt_ctrl={:#x} erst_base={:#x} erst_size={}",
                        u64::from_le_bytes(raw[0..8].try_into().unwrap_or([0; 8])),
                        u32::from_le_bytes(raw[12..16].try_into().unwrap_or([0; 4])),
                        u64::from_le_bytes(erst[0..8].try_into().unwrap_or([0; 8])),
                        u32::from_le_bytes(erst[8..12].try_into().unwrap_or([0; 4]))
                    );"""
new = """                    crate::kwarn!(
                        "xhci: forensic evt_ptr={:#x} evt_ctrl={:#x} erst_base={:#x} erst_size={}",
                        u64::from_le_bytes(raw[0..8].try_into().unwrap_or([0; 8])),
                        u32::from_le_bytes(raw[12..16].try_into().unwrap_or([0; 4])),
                        u64::from_le_bytes(erst[0..8].try_into().unwrap_or([0; 8])),
                        u32::from_le_bytes(erst[8..12].try_into().unwrap_or([0; 4]))
                    );
                    // 全环取证：16 槽 ctrl（事件环）+ cmd 环前 4 槽 ctrl。
                    for slot_i in 0..16usize {
                        let mut sr = [0u8; 32];
                        let sa = self.evt_phys + 64 + (slot_i as u64) * 32;
                        self.mem.read_bytes(sa, 0, &mut sr);
                        let sc = u32::from_le_bytes(sr[12..16].try_into().unwrap_or([0; 4]));
                        if sc != 0 {
                            crate::kwarn!(
                                "xhci: evt_slot[{}] ctrl={:#x} ptr={:#x}",
                                slot_i,
                                sc,
                                u64::from_le_bytes(sr[0..8].try_into().unwrap_or([0; 8]))
                            );
                        }
                    }
                    for slot_i in 0..4usize {
                        let mut sr = [0u8; 32];
                        let sa = self.cmd_phys + (slot_i as u64) * 32;
                        self.mem.read_bytes(sa, 0, &mut sr);
                        crate::kwarn!(
                            "xhci: cmd_slot[{}] ctrl={:#x}",
                            slot_i,
                            u32::from_le_bytes(sr[12..16].try_into().unwrap_or([0; 4]))
                        );
                    }"""
assert s.count(old) == 1, f'count={s.count(old)}'
s = s.replace(old, new)
p.write_text(s, encoding="utf-8")
print("WRITTEN")
