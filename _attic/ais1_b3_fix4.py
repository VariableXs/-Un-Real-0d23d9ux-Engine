# -*- coding: utf-8 -*-
import os
BASE = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\kernel\varix\src\secstar"

def patch(p, subs):
    fp = os.path.join(BASE, p)
    s = open(fp, encoding='utf-8').read()
    for old, new in subs:
        assert old in s, (p, old[:70])
        s = s.replace(old, new, 1)
    open(fp, 'w', encoding='utf-8').write(s)

# diagsnap_b3: drop unneeded mut in delta test fixture
patch('diagsnap_b3.rs', [
  ("""    let mut old = [0u8; 4 * DELTA_BLOCK];
    let mut new = old;""",
   """    let old = [0u8; 4 * DELTA_BLOCK];
    let mut new = old;"""),
])

# memguard_b3: is_frozen superseded by in-freeze lookup — delete dead method
patch('memguard_b3.rs', [
  ("""    fn is_frozen(&self, base: u64) -> bool {
        self.slots.iter().flatten().any(|s| s.base == base)
    }

""",
   """"""),
])

print("warnings cleaned")
