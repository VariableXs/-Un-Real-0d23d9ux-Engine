def patch(path, pairs):
    s = open(path, encoding='utf-8').read()
    for old, new in pairs:
        if old not in s:
            print("MISS %s: %s" % (path, old[:70]))
        s = s.replace(old, new, 1)
    open(path, 'w', encoding='utf-8', newline='\n').write(s)

patch('src/features/u3/deskicons.ts', [
    ('  const h = m[1]!; // 正则已保证捕获组存在\n  if (h.length === 3) h = h.split("").map((c) => c + c).join("");',
     '  let h = m[1]!; // 正则已保证捕获组存在\n  if (h.length === 3) h = h.split("").map((c) => c + c).join("");'),
    ('      while (take < chars.length && used + charWidth(chars[take]) <= space) {\n        used += charWidth(chars[take]);',
     '      while (take < chars.length && used + charWidth(chars[take]!) <= space) {\n        used += charWidth(chars[take]!);'),
])

patch('src/features/u3/explorerx.ts', [
    ('  if (hits.length === 0) return { count: 0, firstContext: "" };\n  const h = hits[0];',
     '  if (hits.length === 0) return { count: 0, firstContext: "" };\n  const h = hits[0]!; // 早退守卫后非空'),
])

patch('src/features/u3/filesec.ts', [
    ('  mode: "overwrite" | "honest-label";\n  passes: number;',
     '  void files; // 策略层不消费文件清单（界面警示层消费）\n  return {\n    mode: "overwrite" | "honest-label";\n    passes: number;' if False else 'export function shredPlan(medium: ShredMediumCapability, files: string[]): { // files 用于界面清单展示（策略层不消费）\n  void files;\n  return {'),
])
print("phase2 done")
