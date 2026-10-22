"""Generates synthetic paper-model sheets with known paint colours, rivets,
panel lines, part outlines, and a white star insignia (run: python3 -I generate.py <dir>)."""
import json, math, sys
import fitz

out = sys.argv[1]
MM = 72 / 25.4
PAINTS = {
    "olive": (0.42, 0.45, 0.28),
    "sand": (0.80, 0.72, 0.52),
    "grey": (0.55, 0.57, 0.60),
    "lightblue": (0.62, 0.74, 0.84),
}
doc = fitz.open()
meta = {"paints": {k: [round(c * 255) for c in v] for k, v in PAINTS.items()}, "pages": []}


def darker(c, f):
    return tuple(x * f for x in c)


def panel(page, rec, name, x, y, w, h):
    """A painted panel with an outline, two panel lines, and rivet rows."""
    paint = PAINTS[name]
    shape = page.new_shape()
    shape.draw_rect(fitz.Rect(x * MM, y * MM, (x + w) * MM, (y + h) * MM))
    shape.finish(color=(0, 0, 0), fill=paint, width=0.3 * MM)
    shape.commit()
    lines = []
    for k in (1, 2):
        lx = x + w * k / 3
        s = page.new_shape()
        s.draw_line(fitz.Point(lx * MM, (y + 2) * MM), fitz.Point(lx * MM, (y + h - 2) * MM))
        s.finish(color=darker(paint, 0.55), width=0.25 * MM)
        s.commit()
        lines.append([lx, y + 2, lx, y + h - 2])
    rivets = []
    for ry in (y + 4, y + h - 4):
        rx = x + 4
        while rx < x + w - 4:
            s = page.new_shape()
            s.draw_circle(fitz.Point(rx * MM, ry * MM), 0.35 * MM)
            s.finish(color=None, fill=darker(paint, 0.6))
            s.commit()
            rivets.append([round(rx, 2), ry])
            rx += 5
    rec["panels"].append({"paint": name, "rect_mm": [x, y, w, h], "lines": lines, "rivets": rivets})


def star(page, rec, cx, cy, r):
    pts = []
    for k in range(10):
        a = -math.pi / 2 + k * math.pi / 5
        rr = r if k % 2 == 0 else r * 0.4
        pts.append(fitz.Point((cx + rr * math.cos(a)) * MM, (cy + rr * math.sin(a)) * MM))
    s = page.new_shape()
    s.draw_polyline(pts + [pts[0]])
    s.finish(color=None, fill=(1, 1, 1), closePath=True)
    s.commit()
    rec["insignia"].append([cx - r, cy - r, 2 * r, 2 * r])


for p in range(2):
    page = doc.new_page(width=297 * MM, height=420 * MM)
    rec = {"panels": [], "insignia": []}
    page.insert_text(fitz.Point(20 * MM, 18 * MM), f"RECOLOR TEST SHEET {p + 1}", fontsize=14, color=(0, 0, 0))
    names = list(PAINTS)
    for i in range(4):
        name = names[(i + p) % 4]
        panel(page, rec, name, 20 + (i % 2) * 135, 40 + (i // 2) * 120, 120, 100)
    # A large olive panel with a white star on it.
    panel(page, rec, "olive", 20, 290, 255, 100)
    star(page, rec, 147, 340, 25)
    meta["pages"].append(rec)

doc.save(out + "/model_paint.pdf", deflate=True)
json.dump(meta, open(out + "/recolor.json", "w"), indent=1)
