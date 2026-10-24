# -*- coding: utf-8 -*-
import io

BASE = 'D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel/varix/src/svstar2/'
Q = chr(39)  # single quote

# ves239
t = io.open(BASE + 'ves239_s10sec.rs', encoding='utf-8').read()
t = t.replace('.filter(|(_, p)| !p).map(|(n, _)| n).collect()',
              '.filter(|(_, p)| !p).map(|(n, _)| *n).collect()')
t = t.replace('.filter(|(_, e)| !e).map(|(n, _)| n).collect()',
              '.filter(|(_, e)| !e).map(|(n, _)| *n).collect()')
t = t.replace('pub fn pref_overreach(writes: &[PrefWriteAudit], authorized: &[&str]) -> Vec<&PrefWriteAudit> {',
              'pub fn pref_overreach<' + Q + 'a>(writes: &' + Q + 'a [PrefWriteAudit], authorized: &[&str]) -> Vec<&' + Q + 'a PrefWriteAudit> {')
io.open(BASE + 'ves239_s10sec.rs', 'w', encoding='utf-8', newline='').write(t)

# ves245
t = io.open(BASE + 'ves245_s10ext.rs', encoding='utf-8').read()
t = t.replace('.filter(|(_, c)| !c.alive()).map(|(s, _)| s).collect()',
              '.filter(|(_, c)| !c.alive()).map(|(s, _)| *s).collect()')
io.open(BASE + 'ves245_s10ext.rs', 'w', encoding='utf-8', newline='').write(t)

# ves248
t = io.open(BASE + 'ves248_s10walk.rs', encoding='utf-8').read()
t = t.replace('name: String::from(t),', 'name: String::from(*t),')
io.open(BASE + 'ves248_s10walk.rs', 'w', encoding='utf-8', newline='').write(t)

# ves251
t = io.open(BASE + 'ves251_s10kb.rs', encoding='utf-8').read()
t = t.replace('keywords: keywords.iter().map(|k| String::from(k)).collect(),',
              'keywords: keywords.iter().map(|k| String::from(*k)).collect(),')
io.open(BASE + 'ves251_s10kb.rs', 'w', encoding='utf-8', newline='').write(t)

# ves267
t = io.open(BASE + 'ves267_s12table.rs', encoding='utf-8').read()
t = t.replace('.map(|(h, a)| Column { header: String::from(h), align_left: *a })',
              '.map(|(h, a)| Column { header: String::from(*h), align_left: *a })')
io.open(BASE + 'ves267_s12table.rs', 'w', encoding='utf-8', newline='').write(t)

print('batch ok')
