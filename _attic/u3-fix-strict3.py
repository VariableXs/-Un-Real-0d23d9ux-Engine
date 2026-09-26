def patch(path, pairs):
    s = open(path, encoding='utf-8').read()
    for old, new in pairs:
        if old not in s:
            print("MISS %s: %s" % (path, old[:70]))
        s = s.replace(old, new, 1)
    open(path, 'w', encoding='utf-8', newline='\n').write(s)

patch('src/features/u3/copyops.ts', [
    ('pass: d1.kind === "corrupt" && d2.kind === "app-missing" && d3.kind === "permission" && d1.what && d1.why && d1.next',
     'pass: d1.kind === "corrupt" && d2.kind === "app-missing" && d3.kind === "permission" && d1.what !== "" && d1.why !== "" && d1.next !== ""'),
])

patch('src/features/u3/U3Runtime.tsx', [
    ('            const delay = starts[i];',
     '            const delay = starts[i] ?? 0;'),
    ('  const [pointerOpacity, setPointerOpacity] = useState(1);',
     '  const [, setPointerOpacity] = useState(1); // 值经 CSS 变量下发（documentElement），组件内不直读'),
])
print("phase3 done")
