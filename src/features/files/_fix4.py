import io

BASE = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/src/features/files/"

def L(p):
    return io.open(BASE + p, encoding='utf-8').read()

def S(p, s):
    io.open(BASE + p, 'w', encoding='utf-8', newline='').write(s)

p = 'checks.ts'
s = L(p)
old_lp = "'" + "\\" * 8 + "?" + "\\" * 4 + "'"
new_lp = "'" + "\\" * 4 + "?" + "\\" * 2 + "'"
assert old_lp in s, 'longpath pattern not found'
s = s.replace(old_lp, new_lp)
old66 = "(bin.restore(v, ['/src/one.txt']), bin.delete(v, '/src/one.txt') !== undefined && bin.restore(v, ['/src/one.txt']) === 1)"
new66 = "(bin.restore(v, 'all'), bin.delete(v, '/src/one.txt') !== undefined && bin.restore(v, ['/src/one.txt']) === 1 && !bin.search('one').length)"
assert old66 in s, 'F03266 pattern not found'
s = s.replace(old66, new66)
old90 = ".includes('" + "\\" + "N')"
new90 = ".includes('A" + "\\\\" + "nB')"
assert old90 in s, 'F03490 pattern not found'
s = s.replace(old90, new90)
assert "includes('0:0:1.50')" in s, 'F03491 pattern not found'
s = s.replace("includes('0:0:1.50')", "includes('0:00:01.50')")
S(p, s)

p = 'groupB.ts'
s = L(p)
old = """    if (node?.exif) {
      const kept = exifClear(node, ['ColorSpace']);
      v.update(p, { exif: kept });"""
new = """    if (node?.exif) {
      const kept: Record<string, string> = {};
      for (const [k, val] of Object.entries(node.exif)) if (k === 'ColorSpace') kept[k] = val;
      v.update(p, { exif: kept });"""
assert old in s, 'privacyScrub pattern not found'
s = s.replace(old, new)
S(p, s)
print('ok')
