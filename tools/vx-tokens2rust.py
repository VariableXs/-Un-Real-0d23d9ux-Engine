#!/usr/bin/env python3
"""Convert src/design/tokens.css OKLCH colors to sRGB and emit Rust token tables.
Faithful port: every oklch() value in tokens.css -> RGBA8 constants."""
import re, sys, math

CSS = open("src/design/tokens.css", encoding="utf-8").read()

def oklch_to_srgb(L, C, H, a=1.0):
    h = math.radians(H)
    lr, lg, lb = L, L, L
    # OKLab: a = C*cos(h), b = C*sin(h)
    A, B = C*math.cos(h), C*math.sin(h)
    l_ = L + 0.3963377774*A + 0.2158037573*B
    m_ = L - 0.1055613458*A - 0.0638541728*B
    s_ = L - 0.0894841775*A - 1.2914855480*B
    l, m, s = l_**3, m_**3, s_**3
    r = +4.0767416621*l - 3.3077115913*m + 0.2309699292*s
    g = -1.2684380046*l + 2.6097574011*m - 0.3413193965*s
    b = -0.0041960863*l - 0.7034186147*m + 1.7076147010*s
    def gam(u):
        u = max(-0.001, min(1.001, u))
        return 12.92*u if u <= 0.0031308 else 1.055*(u**(1/2.4))-0.055
    r, g, b = gam(r), gam(g), gam(b)
    return (round(r*255), round(g*255), round(b*255), round(a*255))

def css_rgba(css, dark=True):
    """Return (name, (r,g,b,a)) resolved for dark layer, tracking alpha."""
    # split at 0.14 alpha etc: parse "oklch(L C H)" or "oklch(L C H / A)"
    pat = re.compile(r"oklch\(\s*([\d.]+)\s+([\d.]+)\s+([\d.]+)(?:\s*/\s*([\d.]+))?\s*\)")
    def rep(mo):
        L, C, H = float(mo[1]), float(mo[2]), float(mo[3])
        a = float(mo[4]) if mo[4] else 1.0
        r, g, b, a8 = oklch_to_srgb(L, C, H, a)
        return f"0x{a8:02X}_{r:02X}_{g:02X}_{b:02X}"
    return pat.sub(rep, css)

out = css_rgba(CSS)
print(out)
