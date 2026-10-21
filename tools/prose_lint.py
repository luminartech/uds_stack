#!/usr/bin/env python3
"""Report comment prose that is written for the author rather than the reader.

Scans comments and doc comments in Rust, Python, C/C++, shell, TOML and YAML
files, and the headings of Markdown files, and counts phrases the writing rules
exclude (see CONTRIBUTING.md, "Writing for the reader"). British spelling is
also checked in the body of every Markdown file but a changelog.

    prose_lint.py [--fail] [--baseline FILE] PATH...

Without --fail or --strict the script prints a report and exits 0. With
--strict PREFIX (repeatable) it exits 1 when a hard-fail phrase appears in a
file under that prefix, so crates that have had their writing pass are gated
while the rest are only reported. --fail gates every path. A baseline file (a
JSON object of soft-signal name to allowed count) turns any soft signal above
its allowance into a failure as well.
"""
import argparse, collections, json, os, re, sys

# Comment syntaxes by extension. Each entry: (line_prefixes, block_pair)
SYNTAX = {
    ".rs":  (("//",), None),
    ".py":  (("#",), ('"""', '"""')),
    ".c":   (("//",), ("/*", "*/")),
    ".h":   (("//",), ("/*", "*/")),
    ".cpp": (("//",), ("/*", "*/")),
    ".hpp": (("//",), ("/*", "*/")),
    ".cc":  (("//",), ("/*", "*/")),
    ".sh":  (("#",), None),
    ".toml": (("#",), None),
    ".yaml": (("#",), None),
    ".yml": (("#",), None),
}
# Markdown is scanned for its headings only, outside fenced code blocks, except for
# British spelling, which is checked in the body too. A changelog's body is history.
MARKDOWN = (".md",)
CHANGELOG = "CHANGELOG.md"
SPELLING = "British spelling"
SKIP_DIRS = {"target", ".git", ".venv", "__pycache__", "_build", "corpus", "artifacts"}

# British spellings, as whole words. The -ise, -lled and -our forms match their
# inflections too.
BRITISH_ISE = """initialise recognise organise synchronise realise standardise finalise
    minimise maximise optimise summarise characterise authorise prioritise utilise customise
    specialise categorise serialise deserialise normalise parameterise generalise stabilise
    tokenise virtualise canonicalise centralise localise visualise sanitise randomise
    memorise criticise apologise""".split()
BRITISH_LLED = """cancelled labelled modelled signalled travelled fuelled levelled marshalled
    channelled totalled""".split()
BRITISH_OUR = """behaviour neighbour colour favour honour labour harbour humour flavour
    endeavour""".split()
BRITISH_WORDS = """acknowledgement acknowledgements judgement judgements artefact artefacts
    catalogue catalogues catalogued analogue programme programmes centre centres centred
    metre metres litre litres fibre fibres defence offence licence licences grey whilst
    amongst fulfil fulfils fulfilment enrolment instalment skilful wilful ageing manoeuvre
    sceptical analyse analysed analysing speciality""".split()
BRITISH = r"\b((un|re)?(%s)(e|es|ed|ing|er|ers|ation|ations|able)|(un)?(%s)(ed|ing)|(%s)\w*|%s)\b" % (
    "|".join(w[:-1] for w in BRITISH_ISE),
    "|".join(w[:-2] for w in BRITISH_LLED),
    "|".join(BRITISH_OUR),
    "|".join(BRITISH_WORDS),
)

HARD = collections.OrderedDict([
    ("load-bearing",            r"load[- ]bearing"),
    ("first person (we/our/us)", r"\b(we|we're|we've|our|ours|us)\b"),
    ("deliberately/intentionally/on purpose", r"\b(deliberate(ly)?|intentional(ly)?|on purpose)\b"),
    ("review-tool or PR/ticket reference", r"(\bCopilot\b|\bPR ?#\d+|\b[A-Za-z_]+#\d{2,6}\b|\(#\d{2,5}\)|\bissue ?#\d+)"),
    ("'What this does not do' heading", r"^(//[/!]|#|\*)\s*#+\s*What\b.*\b(does not|doesn't|is not|isn't|will not|won't)\b"),
    (SPELLING, BRITISH),
])
SOFT = collections.OrderedDict([
    ("history (previously/used to/no longer/is now)", r"\b(previously|used to|no longer|was changed|has been changed|was removed|was added|is now|are now|now (uses|lives|returns|takes|has)|moved (to|from|here)|renamed|replaced by)\b"),
    ("incident (regression/the fix/the bug)", r"\b(regression|the fix|this fix|the bug|bugfix|workaround for)\b"),
    ("editor instruction (do not remove/must stay)", r"\b(do not (remove|delete|change|rename|inline)|don't (remove|delete|change)|must (stay|remain)|keep this)\b"),
    ("hedging (probably/likely/seems)", r"\b(probably|seems to|appears to|most likely|presumably|I think)\b"),
    ("'Why' heading in a doc comment", r"^\s*(//[/!]|#|\*)\s*#+\s*Why\b"),
    ("comment block of 12+ lines", None),
    ("em-dash", r"—"),
])

def without_code(text):
    """Drop code spans, URLs and identifier-shaped words: spelling is checked on prose."""
    return re.sub(r"`[^`]*`|https?://\S+|\]\([^)]*\)|\b\w*([a-z][A-Z]|_)\w*\b", " ", text)

def markdown_body(path):
    """Yield (lineno, text) for every non-heading line outside a fenced code block."""
    if os.path.basename(path) == CHANGELOG:
        return
    try:
        text = open(path, encoding="utf-8", errors="replace").read()
    except OSError:
        return
    fenced = False
    for i, raw in enumerate(text.split("\n"), 1):
        s = raw.strip()
        if s.startswith("```") or s.startswith("~~~"):
            fenced = not fenced
        elif not fenced and s and not re.match(r"#+\s", s):
            yield i, s

def comment_lines(path):
    """Yield (lineno, text) for every comment line in the file."""
    ext = os.path.splitext(path)[1]
    if ext in MARKDOWN:
        yield from markdown_headings(path)
        return
    if ext not in SYNTAX:
        return
    prefixes, block = SYNTAX[ext]
    try:
        text = open(path, encoding="utf-8", errors="replace").read()
    except OSError:
        return
    in_block = False
    for i, raw in enumerate(text.split("\n"), 1):
        s = raw.strip()
        is_comment = False
        if in_block:
            is_comment = True
            if block and block[1] in s:
                in_block = False
        elif block and s.startswith(block[0]):
            is_comment = True
            if block[1] not in s[len(block[0]):]:
                in_block = True
        elif any(s.startswith(p) for p in prefixes):
            is_comment = True
        if is_comment:
            yield i, s

def markdown_headings(path):
    """Yield (lineno, text) for every ATX heading outside a fenced code block."""
    try:
        text = open(path, encoding="utf-8", errors="replace").read()
    except OSError:
        return
    fenced = False
    for i, raw in enumerate(text.split("\n"), 1):
        s = raw.strip()
        if s.startswith("```") or s.startswith("~~~"):
            fenced = not fenced
        elif not fenced and re.match(r"#+\s", s):
            yield i, s

def scan(paths, strict):
    hard = collections.Counter(); soft = collections.Counter()
    hits = collections.defaultdict(list)
    gated = collections.Counter()
    files = 0
    for root in paths:
        if os.path.isfile(root):
            walk = [(os.path.dirname(root) or ".", [], [os.path.basename(root)])]
        else:
            walk = os.walk(root)
        for dp, dn, fn in walk:
            dn[:] = [d for d in dn if d not in SKIP_DIRS]
            for f in fn:
                p = os.path.join(dp, f)
                if os.path.splitext(f)[1] not in SYNTAX and os.path.splitext(f)[1] not in MARKDOWN:
                    continue
                files += 1
                run = 0; run_start = 0; last = -1
                for ln, s in comment_lines(p):
                    if ln == last + 1:
                        run += 1
                    else:
                        if run >= 12:
                            soft["comment block of 12+ lines"] += 1
                            hits["comment block of 12+ lines"].append(f"{p}:{run_start} ({run} lines)")
                        run = 1; run_start = ln
                    last = ln
                    for k, r in HARD.items():
                        if re.search(r, without_code(s) if k == SPELLING else s, re.I):
                            hard[k] += 1; hits[k].append(f"{p}:{ln}: {s[:140]}")
                            if any(os.path.normpath(p).startswith(pre) for pre in strict):
                                gated[k] += 1
                    for k, r in SOFT.items():
                        if r and re.search(r, s, re.I):
                            soft[k] += 1; hits[k].append(f"{p}:{ln}: {s[:140]}")
                if run >= 12:
                    soft["comment block of 12+ lines"] += 1
                    hits["comment block of 12+ lines"].append(f"{p}:{run_start} ({run} lines)")
                if os.path.splitext(f)[1] in MARKDOWN:
                    for ln, s in markdown_body(p):
                        if re.search(HARD[SPELLING], without_code(s), re.I):
                            hard[SPELLING] += 1; hits[SPELLING].append(f"{p}:{ln}: {s[:140]}")
                            if any(os.path.normpath(p).startswith(pre) for pre in strict):
                                gated[SPELLING] += 1
    return files, hard, soft, hits, gated

def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("paths", nargs="+")
    ap.add_argument("--fail", action="store_true", help="exit 1 on a hard-fail phrase anywhere, or a soft signal above the baseline")
    ap.add_argument("--strict", action="append", default=[], metavar="PREFIX", help="exit 1 on a hard-fail phrase in a file under this path prefix (repeatable)")
    ap.add_argument("--baseline", help="JSON file of soft-signal name to allowed count")
    ap.add_argument("--show", type=int, default=0, metavar="N", help="print up to N locations per signal")
    ap.add_argument("--summary", help="append a Markdown summary to this file (GitHub job summary)")
    a = ap.parse_args()
    strict = [os.path.normpath(x) for x in a.strict]
    files, hard, soft, hits, gated = scan(a.paths, strict)
    baseline = json.load(open(a.baseline)) if a.baseline else {}
    rows = []
    failed = False
    for k in HARD:
        bad = hard[k] > 0 if a.fail else gated[k] > 0
        rows.append((k, hard[k], gated[k] if strict and not a.fail else 0, bad)); failed |= bad
    for k in SOFT:
        allowed = baseline.get(k)
        over = allowed is not None and soft[k] > allowed
        rows.append((k, soft[k], allowed, over)); failed |= over
    w = max(len(r[0]) for r in rows)
    print(f"{files} files scanned")
    gate = "gated" if strict and not a.fail else "allowed"
    if strict and not a.fail:
        print("strict under: " + ", ".join(strict))
    print(f"{'signal':{w}}  {'count':>6}  {gate:>7}")
    for k, n, allowed, bad in rows:
        flag = "  FAIL" if bad else ""
        print(f"{k:{w}}  {n:6}  {('-' if allowed is None else allowed):>7}{flag}")
    if a.show:
        for k in list(HARD) + list(SOFT):
            if hits[k]:
                print(f"\n-- {k}")
                for h in hits[k][:a.show]:
                    print("  " + h)
    if a.summary:
        with open(a.summary, "a") as f:
            f.write("### Comment prose\n\n| Signal | Count | Allowed |\n|---|---:|---:|\n")
            for k, n, allowed, bad in rows:
                f.write(f"| {k} | {n}{' ⚠️' if bad else ''} | {'hard fail' if allowed == 0 and k in HARD else ('-' if allowed is None else allowed)} |\n")
    sys.exit(1 if failed else 0)

if __name__ == "__main__":
    main()
