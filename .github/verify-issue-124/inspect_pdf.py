"""Report how every text run in a markdown2pdf PDF is drawn.

For each run: its text, the font that draws it (family and style read
from the embedded font, or the built-in PDF font name), and whether
bold or italic is real (a dedicated face) or synthesized (stroked or
slanted). Words named after a style (`plain`, `bold`, `italic`,
`bolditalic`) are checked against that style.

Usage: inspect_pdf.py LABEL PDF [--require-correct]
Writes a Markdown report to stdout (and $GITHUB_STEP_SUMMARY when set)
and a cropped PNG of page 1 next to the PDF.
"""

import io
import os
import re
import sys

import pypdfium2
from fontTools.ttLib import TTFont
from pypdf import PdfReader
from pypdf.generic import ByteStringObject, ContentStream, TextStringObject

EXPECTED = {
    "plain": (False, False),
    "bold": (True, False),
    "italic": (False, True),
    "bolditalic": (True, True),
}


def parse_to_unicode(stream):
    """Map character codes to text from a simple ToUnicode CMap."""
    data = stream.get_data().decode("latin-1")
    mapping = {}
    for block in re.findall(r"beginbfchar(.*?)endbfchar", data, re.S):
        for src, dst in re.findall(r"<([0-9A-Fa-f]+)>\s*<([0-9A-Fa-f]+)>", block):
            mapping[int(src, 16)] = bytes.fromhex(dst).decode("utf-16-be")
    for block in re.findall(r"beginbfrange(.*?)endbfrange", data, re.S):
        for lo, hi, dst in re.findall(
            r"<([0-9A-Fa-f]+)>\s*<([0-9A-Fa-f]+)>\s*<([0-9A-Fa-f]+)>", block
        ):
            start = int(dst, 16)
            for i, code in enumerate(range(int(lo, 16), int(hi, 16) + 1)):
                mapping[code] = chr(start + i)
    return mapping


def describe_font(font):
    """Family, (bold, italic), and a decoder for one font resource."""
    font = font.get_object()
    if font.get("/Subtype") == "/Type0":
        descendant = font["/DescendantFonts"][0].get_object()
        descriptor = descendant["/FontDescriptor"].get_object()
        tt = TTFont(io.BytesIO(descriptor["/FontFile2"].get_object().get_data()))
        name = tt["name"]
        family = str(name.getBestFamilyName() or "?")
        style = str(name.getBestSubFamilyName() or "")
        mac_style = tt["head"].macStyle
        lowered = style.lower()
        bold = bool(mac_style & 1) or any(
            w in lowered for w in ("bold", "black", "heavy", "semibold")
        )
        if "OS/2" in tt:
            bold = tt["OS/2"].usWeightClass >= 600
        italic = (
            bool(mac_style & 2)
            or "italic" in lowered
            or "oblique" in lowered
            or ("post" in tt and tt["post"].italicAngle != 0)
        )
        cmap = parse_to_unicode(font["/ToUnicode"].get_object())

        def decode(raw):
            return "".join(
                cmap.get(int.from_bytes(raw[i : i + 2], "big"), "?")
                for i in range(0, len(raw) - 1, 2)
            )

        return f"{family} {style}".strip(), (bold, italic), decode, head_fingerprint(tt)
    base = str(font.get("/BaseFont", "?")).lstrip("/")
    bold = "Bold" in base
    italic = "Oblique" in base or "Italic" in base
    return (
        f"{base} (built-in)",
        (bold, italic),
        lambda raw: raw.decode("latin-1"),
        None,
    )


def head_fingerprint(tt):
    """`head` fields subsetting preserves, identifying the source file
    even when the embedded copy lost its family name."""
    head = tt["head"]
    return (head.created, head.modified, head.fontRevision, head.unitsPerEm)


def file_fingerprint(path):
    return head_fingerprint(TTFont(path, lazy=True))


def raw_bytes(item):
    """The undecoded bytes of a string operand; nothing for TJ offsets."""
    if isinstance(item, ByteStringObject):
        return bytes(item)
    if isinstance(item, TextStringObject):
        raw = item.get_original_bytes()
        # pypdf decodes glyph-id strings as UTF-16 and re-adds a BOM.
        if getattr(item, "autodetect_utf16", False) and raw[:2] == b"\xfe\xff":
            raw = raw[2:]
        return raw
    return b""


def runs(pdf):
    page = PdfReader(pdf).pages[0]
    fonts = page["/Resources"]["/Font"]
    cache = {}
    font, mode, shear = None, 0, 0.0
    saved = []
    out = []
    for operands, op in ContentStream(page.get_contents(), page.pdf).operations:
        op = op.decode()
        if op == "q":
            saved.append((font, mode))
        elif op == "Q":
            font, mode = saved.pop()
        elif op == "BT":
            shear = 0.0
        elif op == "Tf":
            font = str(operands[0])
        elif op == "Tr":
            mode = int(operands[0])
        elif op == "Tm":
            shear = float(operands[2])
        elif op in ("Tj", "TJ"):
            items = operands[0] if op == "TJ" else [operands[0]]
            raw = b"".join(raw_bytes(i) for i in items)
            if not raw:
                continue
            if font not in cache:
                cache[font] = describe_font(fonts[font])
            family, (bold, italic), decode, fingerprint = cache[font]
            out.append(
                {
                    "text": decode(raw),
                    "font": family,
                    "real_bold": bold,
                    "real_italic": italic,
                    "fake_bold": mode == 2,
                    "fake_italic": abs(shear) > 0.01,
                    "fingerprint": fingerprint,
                }
            )
    return out


def render_png(pdf):
    page = pypdfium2.PdfDocument(pdf)[0]
    image = page.render(scale=2).to_pil().convert("L")
    box = image.point(lambda p: 255 if p < 250 else 0).getbbox()
    if box:
        pad = 20
        image = image.crop(
            (
                max(box[0] - pad, 0),
                max(box[1] - pad, 0),
                min(box[2] + pad, image.width),
                min(box[3] + pad, image.height),
            )
        )
    path = os.path.splitext(pdf)[0] + ".png"
    image.save(path)
    return path


def main():
    label, pdf = sys.argv[1], sys.argv[2]
    require = "--require-correct" in sys.argv
    lines = [
        f"### {label}",
        "",
        "| text | font | bold | italic | verdict |",
        "|---|---|---|---|---|",
    ]
    failures = []
    for run in runs(pdf):
        bold = "real" if run["real_bold"] else "fake" if run["fake_bold"] else "no"
        italic = (
            "real" if run["real_italic"] else "fake" if run["fake_italic"] else "no"
        )
        word = run["text"].strip().strip(".").lower()
        verdict = ""
        if word in EXPECTED:
            want = EXPECTED[word]
            got = (bold != "no", italic != "no")
            verdict = "ok" if got == want else f"WRONG (expected {word})"
            if got != want:
                failures.append(word)
        lines.append(
            f"| `{run['text']}` | {run['font']} | {bold} | {italic} | {verdict} |"
        )
    lines.append("")
    lines.append(
        f"**{'FAIL' if failures else 'PASS'}**"
        + (f": wrong style for {', '.join(failures)}" if failures else "")
    )
    lines.append(f"\nPNG: `{render_png(pdf)}`\n")
    report = "\n".join(lines)
    print(report)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as f:
            f.write(report + "\n")
    if require and failures:
        sys.exit(1)


if __name__ == "__main__":
    main()
