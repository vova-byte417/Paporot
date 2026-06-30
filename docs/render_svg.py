"""Render basic Excalidraw JSON to SVG without external dependencies.
Handles: text, rectangle, ellipse, line, arrow.
"""
import json
import sys
from pathlib import Path
from xml.etree import ElementTree as ET
import html

def escape(s):
    return html.escape(str(s))

def render_to_svg(data, margin=40):
    elements = data.get("elements", [])
    appState = data.get("appState", {})
    bg = appState.get("viewBackgroundColor", "#ffffff")
    
    # Compute bounding box
    min_x, min_y, max_x, max_y = float("inf"), float("inf"), float("-inf"), float("-inf")
    for el in elements:
        if el.get("isDeleted"):
            continue
        x, y = el.get("x", 0), el.get("y", 0)
        w, h = el.get("width", 0), el.get("height", 0)
        min_x = min(min_x, x)
        min_y = min(min_y, y)
        max_x = max(max_x, x + w if w else x)
        max_y = max(max_y, y + h if h else y)
        # Also consider arrow boundings
        if el.get("type") == "arrow":
            for pt in el.get("points", [[0,0]]):
                px = x + pt[0]
                py = y + pt[1]
                min_x = min(min_x, px)
                min_y = min(min_y, py)
                max_x = max(max_x, px)
                max_y = max(max_y, py)
    
    if min_x == float("inf"):
        min_x, min_y, max_x, max_y = 0, 0, 800, 600
    
    total_w = max_x - min_x + margin * 2
    total_h = max_y - min_y + margin * 2
    
    offset_x = -min_x + margin
    offset_y = -min_y + margin
    
    ns = "http://www.w3.org/2000/svg"
    
    def tx(x): return x + offset_x
    def ty(y): return y + offset_y
    
    svg = ET.Element("svg", {
        "xmlns": ns,
        "width": str(int(total_w)),
        "height": str(int(total_h)),
        "viewBox": f"0 0 {int(total_w)} {int(total_h)}"
    })
    
    # Background
    bg_rect = ET.SubElement(svg, "rect", {
        "width": str(int(total_w)),
        "height": str(int(total_h)),
        "fill": bg
    })
    
    # Sort: arrows/lines first, then shapes, then text on top
    type_order = {"line": 0, "arrow": 1, "rectangle": 2, "ellipse": 3, "text": 4}
    sorted_els = sorted(elements, key=lambda e: type_order.get(e.get("type", ""), 5))
    
    def get_stroke_style(el):
        sw = el.get("strokeWidth", 2)
        style = el.get("strokeStyle", "solid")
        color = el.get("strokeColor", "#000000")
        dash = "none"
        if style == "dashed":
            dash = f"{sw * 3},{sw * 3}"
        return color, sw, dash
    
    for el in sorted_els:
        if el.get("isDeleted"):
            continue
        el_type = el.get("type", "")
        x = tx(el.get("x", 0))
        y = ty(el.get("y", 0))
        w = el.get("width", 0)
        h = el.get("height", 0)
        
        if el_type == "rectangle":
            fill = el.get("backgroundColor", "transparent")
            stroke, sw, dash = get_stroke_style(el)
            r = el.get("roundness", None)
            rect = ET.SubElement(svg, "rect", {
                "x": str(x), "y": str(y),
                "width": str(w), "height": str(h),
                "fill": fill,
                "stroke": stroke,
                "stroke-width": str(sw),
                "stroke-dasharray": dash
            })
            if r:
                rect.set("rx", str(r.get("type", 0) if isinstance(r, dict) else r))
        
        elif el_type == "ellipse":
            fill = el.get("backgroundColor", "transparent")
            stroke, sw, dash = get_stroke_style(el)
            cx, cy = x + w/2, y + h/2
            rx, ry = w/2, h/2
            ET.SubElement(svg, "ellipse", {
                "cx": str(cx), "cy": str(cy),
                "rx": str(rx), "ry": str(ry),
                "fill": fill,
                "stroke": stroke,
                "stroke-width": str(sw),
                "stroke-dasharray": dash
            })
        
        elif el_type in ("line", "arrow"):
            stroke, sw, dash = get_stroke_style(el)
            points = el.get("points", [[0,0]])
            if len(points) < 2:
                continue
            pts_str = " ".join(f"{tx(el['x']+p[0])},{ty(el['y']+p[1])}" for p in points)
            line_el = ET.SubElement(svg, "polyline", {
                "points": pts_str,
                "fill": "none",
                "stroke": stroke,
                "stroke-width": str(sw),
                "stroke-dasharray": dash,
            })
            # Arrowhead for arrow type
            if el_type == "arrow" and len(points) >= 2:
                p1 = points[-2]
                p2 = points[-1]
                dx = p2[0] - p1[0]
                dy = p2[1] - p1[1]
                length = (dx**2 + dy**2)**0.5
                if length > 0:
                    ux, uy = dx/length, dy/length
                    # Arrowhead size
                    ah = 8
                    tip_x = tx(el["x"] + p2[0])
                    tip_y = ty(el["y"] + p2[1])
                    ah1 = (tip_x - ah * ux + ah * 0.4 * uy, tip_y - ah * uy - ah * 0.4 * ux)
                    ah2 = (tip_x - ah * ux - ah * 0.4 * uy, tip_y - ah * uy + ah * 0.4 * ux)
                    pts = f"{tip_x},{tip_y} {ah1[0]},{ah1[1]} {ah2[0]},{ah2[1]}"
                    ET.SubElement(svg, "polygon", {
                        "points": pts,
                        "fill": stroke,
                        "stroke": "none"
                    })
                    line_el.set("marker-end", f"url(#arrow_{el.get('id','')})")
        
        elif el_type == "text":
            text_str = el.get("text", "")
            if not text_str:
                continue
            color = el.get("strokeColor", "#000000")
            font_size = el.get("fontSize", 14)
            text_align = el.get("textAlign", "left")
            v_align = el.get("verticalAlign", "top")
            
            lines = text_str.split("\n")
            line_height = font_size * (el.get("lineHeight", 1.25))
            
            for i, line in enumerate(lines):
                line_y = y + i * line_height
                if v_align == "middle":
                    total_h = len(lines) * line_height
                    line_y = y + (h - total_h) / 2 + i * line_height
                
                anchor = "start"
                text_x = x
                if text_align == "center":
                    anchor = "middle"
                    text_x = x + w / 2
                elif text_align == "right":
                    anchor = "end"
                    text_x = x + w
                
                ET.SubElement(svg, "text", {
                    "x": str(text_x),
                    "y": str(line_y + font_size * 0.85),
                    "fill": color,
                    "font-size": str(font_size),
                    "font-family": "monospace",
                    "text-anchor": anchor,
                }).text = escape(line)
    
    ET.indent(svg)
    return ET.tostring(svg, encoding="unicode")


def main():
    parser = __import__("argparse").ArgumentParser()
    parser.add_argument("input", type=Path)
    parser.add_argument("--output", "-o", type=Path, default=None)
    args = parser.parse_args()
    
    with open(args.input) as f:
        data = json.load(f)
    
    svg_str = render_to_svg(data)
    
    output = args.output or args.input.with_suffix(".svg")
    with open(output, "w", encoding="utf-8") as f:
        f.write(svg_str)
    print(str(output))


if __name__ == "__main__":
    main()
