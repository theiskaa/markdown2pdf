"""End-to-end verification for issue #124 (bold/italic lost on Windows).

Renders a set of scenarios with each given markdown2pdf binary, inspects
how every word is drawn (see inspect_pdf.py), and writes a report.

Exit status is non-zero when the fix gets any scenario wrong, or when
the released v1.6.1 does NOT reproduce the bug on Windows or macOS
(a verification that can't reproduce the bug proves nothing).

Usage:
  verify.py --out DIR --bin LABEL=PATH [--bin ...] [--fix LABEL]
            [--only NAME,NAME] [--sha SHA]
"""

import argparse
import glob
import json
import os
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from inspect_pdf import EXPECTED, file_fingerprint, render_png, runs  # noqa: E402

SYSTEM = platform.system()  # Windows, Darwin, Linux
ISSUE = "**bold** *italic*."
STYLES = "plain\n\n**bold**\n\n*italic*\n\n***bolditalic***"
HEADINGS = "# bold\n\nplain\n\n## bold\n\nplain"
CODE = "`plain`\n\n**`bold`**\n\n*`italic`*\n\n***`bolditalic`***"
UNICODE_TEXT = "café naïve — “quoted” ‘single’ …"
UNICODE = f"Plain {UNICODE_TEXT}"

# Fonts the default body font is picked from ship real bold and italic
# faces on Windows (Segoe UI) and macOS (Arial). Linux's DejaVu Sans
# often lacks an oblique face, so only visible emphasis is required.
DEFAULT_HAS_REAL_FACES = SYSTEM in ("Windows", "Darwin")


def font_file(*candidates):
    """First existing font file among `candidates` (globs allowed)."""
    roots = {
        "Windows": [r"C:\Windows\Fonts"],
        "Darwin": ["/System/Library/Fonts/Supplemental", "/System/Library/Fonts"],
        "Linux": ["/usr/share/fonts"],
    }[SYSTEM]
    for root in roots:
        for name in candidates:
            hits = glob.glob(os.path.join(root, "**", name), recursive=True)
            if hits:
                return hits[0]
    return None


# (font name, file proving it is installed, real bold, real italic)
BODY_FONTS = {
    "Windows": [
        ("Segoe UI", "segoeui.ttf", True, True),
        ("Arial", "arial.ttf", True, True),
        ("Times New Roman", "times.ttf", True, True),
        ("Georgia", "georgia.ttf", True, True),
        ("Verdana", "verdana.ttf", True, True),
        ("Calibri", "calibri.ttf", True, True),
        ("Trebuchet MS", "trebuc.ttf", True, True),
        ("Tahoma", "tahoma.ttf", True, False),
    ],
    "Darwin": [
        ("Arial", "Arial.ttf", True, True),
        ("Times New Roman", "Times New Roman.ttf", True, True),
        ("Georgia", "Georgia.ttf", True, True),
        ("Verdana", "Verdana.ttf", True, True),
        ("Trebuchet MS", "Trebuchet MS.ttf", True, True),
        ("Tahoma", "Tahoma.ttf", True, False),
        ("Geneva", "Geneva.ttf", False, False),
    ],
    "Linux": [
        ("DejaVu Sans", "DejaVuSans.ttf", True, False),
        ("Liberation Sans", "LiberationSans-Regular.ttf", True, True),
    ],
}[SYSTEM]

CODE_FONTS = {
    "Windows": [
        ("Consolas", "consola.ttf", True, True),
        ("Courier New", "cour.ttf", True, True),
    ],
    "Darwin": [("Courier New", "Courier New.ttf", True, True)],
    "Linux": [("DejaVu Sans Mono", "DejaVuSansMono.ttf", True, False)],
}[SYSTEM]


def scenarios():
    """(name, cli args or a cmd.exe line, checks) for this OS."""
    real = (DEFAULT_HAS_REAL_FACES, DEFAULT_HAS_REAL_FACES)
    out = [
        ("issue command", ["-s", ISSUE], {"real": real}),
        ("styles", ["-s", STYLES], {"real": real}),
        ("headings", ["-s", HEADINGS], {"real": real}),
        ("inline code", ["-s", CODE], {}),
        ("unicode text", ["-s", UNICODE], {"text": UNICODE_TEXT}),
    ]
    if SYSTEM == "Windows":
        # Exactly as typed in the issue, through cmd.exe.
        out.insert(1, ("issue via cmd.exe", "CMD", {"real": real}))
    for theme in ["default", "github", "academic", "minimal", "compact", "modern"]:
        out.append((f"theme {theme}", ["--theme", theme, "-s", STYLES], {"real": real}))
    for name, file, bold, italic in BODY_FONTS:
        checks = {"family": name, "real": (bold, italic), "installed": file}
        out.append((f"font {name}", ["--default-font", name, "-s", STYLES], checks))
    for name, file, bold, italic in CODE_FONTS:
        checks = {"family": name, "real": (bold, italic), "installed": file}
        out.append((f"code font {name}", ["--code-font", name, "-s", CODE], checks))
    return out


def render(binary, args, pdf, workdir, env):
    """Run one render; returns (ok, seconds, stderr)."""
    start = time.perf_counter()
    if args == "CMD":
        line = f'"{binary}" -s "{ISSUE}" -o "{pdf}"'
        proc = subprocess.run(
            line, shell=True, cwd=workdir, env=env, capture_output=True, text=True
        )
    else:
        proc = subprocess.run(
            [binary, *args, "-o", str(pdf)],
            cwd=workdir,
            env=env,
            capture_output=True,
            text=True,
        )
    elapsed = time.perf_counter() - start
    return proc.returncode == 0 and pdf.exists(), elapsed, proc.stderr.strip()


def check(pdf, checks):
    """Problems with how `pdf` is drawn, as a list of strings."""
    problems = []
    found = runs(str(pdf))
    want_bold_real, want_italic_real = checks.get("real", (False, False))
    for run in found:
        word = run["text"].strip().strip(".").lower()
        if word not in EXPECTED:
            continue
        want_bold, want_italic = EXPECTED[word]
        got_bold = run["real_bold"] or run["fake_bold"]
        got_italic = run["real_italic"] or run["fake_italic"]
        if (got_bold, got_italic) != (want_bold, want_italic):
            problems.append(f"`{word}` drawn as {style_name(got_bold, got_italic)}")
            continue
        if want_bold and want_bold_real and not run["real_bold"]:
            problems.append(f"`{word}` uses synthesized bold")
        if want_italic and want_italic_real and not run["real_italic"]:
            problems.append(f"`{word}` uses synthesized italic")
    family = checks.get("family")
    if family:
        # Some fonts (Geneva) carry their family name only in Mac name
        # records, which subsetting drops; match those by fingerprint
        # against the installed file instead.
        installed = font_file(checks["installed"])
        expected_print = file_fingerprint(installed) if installed else None
        wrong = {
            r["font"]
            for r in found
            if not r["font"].lower().startswith(family.lower())
            and not (r["fingerprint"] and r["fingerprint"] == expected_print)
        }
        if wrong:
            problems.append(f"not drawn in {family}: {', '.join(sorted(wrong))}")
    text = checks.get("text")
    if text:
        drawn = "".join(r["text"] for r in found)
        if text not in drawn:
            problems.append(f"text drawn as `{drawn}`")
    return problems, found


def style_name(bold, italic):
    return {
        (False, False): "plain",
        (True, False): "bold",
        (False, True): "italic",
        (True, True): "bold italic",
    }[(bold, italic)]


def describe(found):
    return "; ".join(
        f"`{r['text']}` {r['font']}"
        + (" +stroke" if r["fake_bold"] else "")
        + (" +slant" if r["fake_italic"] else "")
        for r in found
        if r["text"].strip()
    )


def compose(out, rows, path):
    """Stack labelled PNGs vertically into one comparison image."""
    from PIL import Image, ImageDraw, ImageFont

    images = [(label, Image.open(png).convert("RGB")) for label, png in rows if png]
    if not images:
        return
    try:
        font = ImageFont.load_default(size=28)
    except TypeError:
        font = ImageFont.load_default()
    width = max(i.width for _, i in images) + 20
    height = sum(i.height + 56 for _, i in images)
    canvas = Image.new("RGB", (width, height), "white")
    draw = ImageDraw.Draw(canvas)
    y = 0
    for label, image in images:
        draw.text((10, y + 10), label, fill=(190, 0, 0), font=font)
        canvas.paste(image, (10, y + 50))
        y += image.height + 56
    canvas.save(path)


def main():
    # The Windows console defaults to cp1252, which can't print the report.
    sys.stdout.reconfigure(encoding="utf-8")
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", required=True)
    parser.add_argument("--bin", action="append", required=True)
    parser.add_argument("--fix", default="fix")
    parser.add_argument("--only")
    parser.add_argument("--sha", default="")
    parser.add_argument("--timing-runs", type=int, default=5)
    opts = parser.parse_args()

    out = Path(opts.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    binaries = dict(b.split("=", 1) for b in opts.bin)
    binaries = {k: str(Path(v).resolve()) for k, v in binaries.items()}

    # A clean working directory and config home, so no project or user
    # config can choose fonts: only the built-in defaults apply.
    workdir = Path(tempfile.mkdtemp(prefix="m2p-verify-"))
    shutil.copy(HERE / "sample.md", workdir / "sample.md")
    env = dict(os.environ)
    env.pop("MARKDOWN2PDF_CONFIG", None)
    env["XDG_CONFIG_HOME"] = str(workdir / "config")

    selected = scenarios()
    selected.append(("sample document", ["-p", "sample.md"], {}))
    if opts.only:
        keep = set(opts.only.split(","))
        selected = [s for s in selected if s[0] in keep]

    results = []
    for name, args, checks in selected:
        installed = checks.get("installed")
        if installed and not font_file(installed):
            results.append({"scenario": name, "skipped": f"{installed} not installed"})
            continue
        row = {"scenario": name, "by_binary": {}}
        for label, binary in binaries.items():
            slug = "".join(c if c.isalnum() else "-" for c in f"{label}-{name}")
            pdf = out / f"{slug}.pdf"
            ok, seconds, stderr = render(binary, args, pdf, workdir, env)
            if not ok:
                row["by_binary"][label] = {"problems": [f"render failed: {stderr}"]}
                continue
            problems, found = check(pdf, checks)
            row["by_binary"][label] = {
                "problems": problems,
                "drawn": describe(found),
                "png": render_png(str(pdf)),
                "seconds": seconds,
            }
        results.append(row)

    # Timing: repeat the issue command per binary.
    timings = {}
    for label, binary in binaries.items():
        samples = []
        for i in range(opts.timing_runs):
            ok, seconds, _ = render(
                binary, ["-s", ISSUE], workdir / f"timing-{i}.pdf", workdir, env
            )
            if ok:
                samples.append(seconds)
        if samples:
            timings[label] = statistics.median(samples)

    # Verdicts.
    errors = []
    for row in results:
        fix = row.get("by_binary", {}).get(opts.fix)
        if fix and fix["problems"]:
            errors.append(f"fix fails `{row['scenario']}`: {'; '.join(fix['problems'])}")
    repro = None
    if SYSTEM in ("Windows", "Darwin") and "v1.6.1" in binaries:
        issue = next((r for r in results if r["scenario"] == "issue command"), None)
        if issue:
            repro = bool(issue["by_binary"]["v1.6.1"]["problems"])
            if not repro:
                errors.append("released v1.6.1 did not reproduce the bug")

    # Report.
    labels = list(binaries)
    lines = [
        f"## Issue #124 verification on {SYSTEM} ({platform.platform()})",
        "",
        f"Fix commit: `{opts.sha or 'n/a'}`. Every render ran in an empty directory "
        "with no config, so only built-in defaults choose fonts.",
        "",
        "| scenario | " + " | ".join(labels) + " |",
        "|---|" + "---|" * len(labels),
    ]
    for row in results:
        if "skipped" in row:
            lines.append(f"| {row['scenario']} | " + " | ".join(["skipped"] * len(labels)) + f" ({row['skipped']}) |")
            continue
        cells = []
        for label in labels:
            r = row["by_binary"].get(label)
            if r is None:
                cells.append("–")
            elif r["problems"]:
                cells.append("❌ " + "; ".join(r["problems"]))
            else:
                cells.append("✅")
        lines.append(f"| {row['scenario']} | " + " | ".join(cells) + " |")
    lines += ["", "### How each word is drawn", ""]
    for row in results:
        if "skipped" in row:
            continue
        lines.append(f"**{row['scenario']}**")
        for label in labels:
            r = row["by_binary"].get(label)
            if r and "drawn" in r:
                lines.append(f"- {label}: {r['drawn']}")
        lines.append("")
    if timings:
        lines += ["### Render time (issue command, median)", ""]
        lines += [f"- {k}: {v * 1000:.0f} ms" for k, v in timings.items()]
        lines.append("")
    if repro is not None:
        lines.append(f"Released v1.6.1 reproduces the bug here: **{'yes' if repro else 'NO'}**")
    lines.append("")
    lines.append("**RESULT: " + ("FAIL**\n\n- " + "\n- ".join(errors) if errors else "PASS**"))
    report = "\n".join(lines)
    (out / "report.md").write_text(report, encoding="utf-8")
    (out / "results.json").write_text(json.dumps(results, indent=2), encoding="utf-8")
    print(report)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as f:
            f.write(report + "\n")

    # One picture per OS: the issue command and the sample, old vs fix.
    rows = []
    for scenario in ("issue command", "sample document"):
        row = next((r for r in results if r["scenario"] == scenario), None)
        if not row or "by_binary" not in row:
            continue
        for label in labels:
            r = row["by_binary"].get(label, {})
            rows.append((f"{SYSTEM}: {label}, {scenario}", r.get("png")))
    compose(out, rows, out / f"compare-{SYSTEM.lower()}.png")

    sys.exit(1 if errors else 0)


if __name__ == "__main__":
    main()
