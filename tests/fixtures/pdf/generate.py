"""Generates synthetic A3 paper-model sheets with known part rectangles."""
import json, sys, math
import fitz

out = sys.argv[1]
MM = 72 / 25.4  # points per mm
doc = fitz.open()
meta = {"pages": []}

def poly(page, pts, fill, width=0.3):
    shape = page.new_shape()
    shape.draw_polyline([fitz.Point(x * MM, y * MM) for x, y in pts] + [fitz.Point(pts[0][0] * MM, pts[0][1] * MM)])
    shape.finish(color=(0, 0, 0), fill=fill, width=width * MM, closePath=True)
    shape.commit()

def dashed(page, a, b):
    shape = page.new_shape()
    shape.draw_line(fitz.Point(a[0] * MM, a[1] * MM), fitz.Point(b[0] * MM, b[1] * MM))
    shape.finish(color=(0.2, 0.2, 0.2), width=0.2 * MM, dashes="[3 2] 0")
    shape.commit()

def part(page, rec, x, y, w, h, fill, label, tabs=True, taper=0.0, center_label=False):
    """A body panel w×h mm at (x, y) with glue tabs on top, a fold line, and a number."""
    body = [(x + taper, y), (x + w - taper, y), (x + w, y + h), (x, y + h)]
    xs, ys = [p[0] for p in body], [p[1] for p in body]
    if tabs:
        tw = min(12.0, w / 4)
        for k in range(int(w // (tw * 1.6))):
            tx = x + taper + 3 + k * tw * 1.6
            if tx + tw > x + w - taper - 3:
                break
            t = [(tx, y), (tx + 2, y - 5), (tx + tw - 2, y - 5), (tx + tw, y)]
            poly(page, t, (1, 1, 1))
            ys.append(y - 5)
    poly(page, body, fill)
    dashed(page, (x + w * 0.5, y + 1), (x + w * 0.5, y + h - 1))
    if center_label:
        # Unfilled parts often carry their number in the middle, far from any line.
        page.insert_text(fitz.Point((x + w * 0.25) * MM, (y + h * 0.55) * MM), label, fontsize=14, color=(0, 0, 0))
    else:
        page.insert_text(fitz.Point((x + 2.5) * MM, (y + h - 2.5) * MM), label, fontsize=9, color=(0, 0, 0))
    rec.append({"label": label, "rect_mm": [min(xs), min(ys), max(xs) - min(xs), max(ys) - min(ys)]})

grey, olive, sand = (0.78, 0.8, 0.82), (0.55, 0.6, 0.42), (0.86, 0.8, 0.62)

# Page 1: fuselage panels, small parts, a title block.
page = doc.new_page(width=297 * MM, height=420 * MM)
rec = []
page.insert_text(fitz.Point(20 * MM, 18 * MM), "TEST AIRCRAFT  1:33  SHEET 1", fontsize=16, color=(0, 0, 0))
part(page, rec, 20, 40, 180, 60, grey, "1")
part(page, rec, 20, 120, 150, 50, grey, "2", taper=12)
part(page, rec, 210, 40, 60, 90, olive, "3")
part(page, rec, 200, 150, 70, 40, sand, "4")
for i in range(6):
    part(page, rec, 20 + i * 44, 200, 36, 26, olive, str(5 + i), tabs=(i % 2 == 0))
part(page, rec, 20, 250, 120, 140, grey, "11")
part(page, rec, 160, 260, 110, 70, sand, "12", taper=8)
meta["pages"].append({"title_mm": [18, 10, 120, 12], "parts": rec})

# Page 2 (landscape A3): a wing longer than A4 in both orientations, a long
# spine that fits only turned 90°, and small parts.
page = doc.new_page(width=420 * MM, height=297 * MM)
rec = []
page.insert_text(fitz.Point(20 * MM, 18 * MM), "TEST AIRCRAFT  1:33  SHEET 2", fontsize=16, color=(0, 0, 0))
part(page, rec, 20, 40, 340, 70, olive, "20 WING")    # 340 × 75: too big for A4 (printable 200 × 287)
part(page, rec, 20, 135, 260, 30, grey, "21")         # 260 × 35: fits only turned 90°
part(page, rec, 300, 135, 90, 90, sand, "22")
part(page, rec, 20, 190, 150, 80, olive, "23", taper=20)
for i in range(3):
    part(page, rec, 185 + i * 38, 245, 32, 30, grey, str(24 + i))
# An unfilled (white) part with its number printed in the middle.
part(page, rec, 310, 240, 70, 45, (1, 1, 1), "27", tabs=False, center_label=True)
meta["pages"].append({"title_mm": [18, 10, 120, 12], "parts": rec})

doc.save(out + "/model_a3.pdf", deflate=True)
json.dump(meta, open(out + "/model_a3.json", "w"), indent=1)
