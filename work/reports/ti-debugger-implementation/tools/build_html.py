"""Build REPORT.html from REPORT.md (report tooling only, not product code).
Run: python3 -I work/reports/ti-debugger-implementation/tools/build_html.py
Supports the Markdown subset REPORT.md uses: headings, tables, nested '-'/'1.' lists, fenced code, hr,
inline code/bold/links. Inserts the SVG figures from figs.py after selected headings."""
import html
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, HERE)
import figs  # noqa: E402

FIG_AFTER = {
    "### 2.1": ("deployment", "그림 1. 배치 — 각 구성요소의 위치와 경계 [제안]"),
    "### 5.4": ("sequence", "그림 4. watch · STOP · deadline 시퀀스 (bridge-session.jsonl / bridge-deadline.jsonl 예시와 같은 흐름) [제안]"),
    "### 7.1": ("layout", "그림 2. Bench Light 워크벤치 와이어프레임 — 보고서 테마와 무관하게 항상 라이트로 표시. 아래 입력/콘솔 줄만 선택형 다크 예시. 제안 개념도, 실제 프로브 화면 아님"),
    "### 7.3": ("states", "그림 3. run-state 상태 기계와 epoch/revision 규칙 [제안]"),
    "### 9.1": ("tickets", "그림 5. 티켓 의존 그래프"),
}
REPLACE_CODE_AFTER = {"### 9.1"}  # the ASCII graph is replaced by the SVG in HTML

LINK_MAP = {"../ti-debugger-survey/SURVEY.md": "../ti-debugger-survey/SURVEY.html"}


def inline(s: str) -> str:
    codes = []

    def keep(m):
        codes.append(f"<code>{html.escape(m.group(1))}</code>")
        return f"\x00{len(codes) - 1}\x00"

    s = re.sub(r"`([^`]+)`", keep, s)
    e = html.escape(s, quote=False)
    e = e.replace("&lt;br&gt;", "<br>")
    e = re.sub(r"\*\*(.+?)\*\*", r"<strong>\1</strong>", e)

    def lk(m):
        text, url = m.group(1), m.group(2)
        url = LINK_MAP.get(url, url)
        return f'<a href="{html.escape(url)}">{text}</a>'

    e = re.sub(r"\[([^\]]+)\]\(([^)\s]+)\)", lk, e)
    e = re.sub(r'(?<!["=>])(https?://[^\s<)]+)', r'<a href="\1">\1</a>', e)
    e = e.replace("✅", '<span class="st ok">✅</span>').replace("⚠️", '<span class="st warn">⚠️</span>').replace("❌", '<span class="st no">❌</span>')
    return re.sub(r"\x00(\d+)\x00", lambda m: codes[int(m.group(1))], e)


def slug(h: str, n: int) -> str:
    m = re.match(r"(\d+(?:\.\d+)?)", h)
    return "s" + (m.group(1).replace(".", "-") if m else str(n))


def convert(md: str):
    lines = md.split("\n")
    out, toc = [], []
    i, n = 0, 0
    pending_fig_skip_code = False
    while i < len(lines):
        ln = lines[i]
        if ln.startswith("```"):
            buf = []
            i += 1
            while i < len(lines) and not lines[i].startswith("```"):
                buf.append(lines[i])
                i += 1
            i += 1
            if pending_fig_skip_code:
                pending_fig_skip_code = False
                continue
            out.append("<pre><code>" + html.escape("\n".join(buf)) + "</code></pre>")
            continue
        m = re.match(r"^(#{1,3}) (.*)$", ln)
        if m:
            lvl, text = len(m.group(1)), m.group(2)
            n += 1
            if lvl == 1:
                out.append(f"<h1>{inline(text)}</h1>")
            else:
                sid = slug(text, n)
                if lvl == 2:
                    toc.append((sid, text))
                out.append(f'<h{lvl} id="{sid}">{inline(text)}</h{lvl}>')
            key = next((k for k in FIG_AFTER if ln.startswith(k + " ")), None)
            if key:
                fn, cap = FIG_AFTER[key]
                fcls = "fig benchlight" if fn == "layout" else "fig"
                out.append(f'<figure class="{fcls}"><div class="figscroll">{getattr(figs, fn)()}</div><figcaption><span class="swipe">↔ 그림을 옆으로 밀어 전체 보기 · </span>{cap}</figcaption></figure>')
                pending_fig_skip_code = key in REPLACE_CODE_AFTER
            i += 1
            continue
        if ln.strip() == "---":
            out.append("<hr>")
            i += 1
            continue
        if ln.lstrip().startswith("|"):
            rows = []
            while i < len(lines) and lines[i].lstrip().startswith("|"):
                rows.append(lines[i])
                i += 1
            cells = lambda r: [c.strip().replace("\x01", "|") for c in r.strip().replace("\\|", "\x01").strip("|").split("|")]
            head = cells(rows[0])
            body = [cells(r) for r in rows[2:]]
            t = ['<div class="tw"><table><thead><tr>' + "".join(f"<th>{inline(c)}</th>" for c in head) + "</tr></thead><tbody>"]
            for r in body:
                t.append("<tr>" + "".join(f"<td>{inline(c)}</td>" for c in r) + "</tr>")
            t.append("</tbody></table></div>")
            out.append("".join(t))
            continue
        if re.match(r"^\s*(- |\d+\. )", ln):
            items = []
            while i < len(lines) and (re.match(r"^\s*(- |\d+\. )", lines[i]) or (lines[i].startswith("  ") and lines[i].strip() and not lines[i].strip().startswith(("```", "|")))):
                items.append(lines[i])
                i += 1
            # fenced code nested inside a list item
            out.append(render_list(items))
            while i < len(lines) and lines[i].startswith("  ```"):
                buf = []
                i += 1
                while i < len(lines) and not lines[i].strip().startswith("```"):
                    buf.append(lines[i][2:] if lines[i].startswith("  ") else lines[i])
                    i += 1
                i += 1
                out.append('<pre class="inlist"><code>' + html.escape("\n".join(buf)) + "</code></pre>")
                # continuation list items after nested code
                items = []
                while i < len(lines) and re.match(r"^\s*(- |\d+\. )", lines[i]):
                    items.append(lines[i])
                    i += 1
                if items:
                    out.append(render_list(items))
            continue
        if not ln.strip():
            i += 1
            continue
        para = [ln]
        i += 1
        while i < len(lines) and lines[i].strip() and not re.match(r"^(#|\||- |\d+\. |```|---)", lines[i]):
            para.append(lines[i])
            i += 1
        out.append("<p>" + inline(" ".join(para)) + "</p>")
    return "\n".join(out), toc


def render_list(items):
    parsed = []
    for it in items:
        m = re.match(r"^(\s*)(- |\d+\. )(.*)$", it)
        if m:
            parsed.append([len(m.group(1)) // 2, "ol" if m.group(2)[0].isdigit() else "ul", m.group(3)])
        elif parsed:
            parsed[-1][2] += " " + it.strip()
    html_out, stack = [], []
    base = parsed[0][0] if parsed else 0
    for depth, kind, text in parsed:
        depth = max(0, depth - base)
        while len(stack) > depth + 1:
            html_out.append(f"</li></{stack.pop()}>")
        if len(stack) == depth + 1:
            html_out.append("</li>")
        while len(stack) < depth + 1:
            html_out.append(f"<{kind}>")
            stack.append(kind)
        html_out.append(f"<li>{inline(text)}")
    while stack:
        html_out.append(f"</li></{stack.pop()}>")
    return "".join(html_out)


CSS = """
:root{color-scheme:light;--bg:#f7f6f2;--fg:#1c1d1f;--muted:#5d5f63;--line:#d8d5cc;--card:#ffffff;--code:#efece4;--ok:#1d7448;--ok-bg:#e1f2e8;--no:#b2261e;--no-bg:#fbe6e3;--warn:#8a5900;--warn-bg:#fcefd4;--acc:#2a5db0;--acc-bg:#e4ecf8;--zone:#f1efe8;--ink2:#3a3c40}
:root[data-theme="dark"]{color-scheme:dark;--bg:#15171a;--fg:#e8e9ea;--muted:#a1a4a8;--line:#373a40;--card:#1e2024;--code:#26292e;--ok:#6fd19c;--ok-bg:#1b3527;--no:#ff8a80;--no-bg:#40211f;--warn:#f1c26a;--warn-bg:#3a2e17;--acc:#8fb2f0;--acc-bg:#1e2940;--zone:#1a1c20;--ink2:#c9cbce}
*{box-sizing:border-box}
html{-webkit-text-size-adjust:100%}
body{margin:0;background:var(--bg);color:var(--fg);font:15px/1.62 -apple-system,BlinkMacSystemFont,"Apple SD Gothic Neo","Noto Sans KR",system-ui,sans-serif}
.wrap{max-width:1120px;margin:0 auto;padding:24px 16px 72px}
h1{font-size:1.65rem;line-height:1.3;margin:4px 0 12px}
h2{font-size:1.25rem;margin:44px 0 12px;padding-bottom:6px;border-bottom:1px solid var(--line);scroll-margin-top:12px}
h3{font-size:1.05rem;margin:26px 0 8px;scroll-margin-top:12px}
p,li{overflow-wrap:anywhere}
ul,ol{padding-left:1.3em;margin:6px 0 12px}
li{margin:2px 0}
hr{border:0;border-top:1px solid var(--line);margin:28px 0}
a{color:var(--acc)}
code,pre{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:.82rem}
code{background:var(--code);padding:1px 4px;border-radius:4px}
pre{background:var(--code);padding:12px 14px;border-radius:8px;overflow-x:auto;line-height:1.45;border:1px solid var(--line)}
pre code{background:none;padding:0}
pre.inlist{margin:4px 0 10px 1.3em}
.tw{overflow-x:auto;border:1px solid var(--line);border-radius:10px;background:var(--card);margin:10px 0 16px}
table{border-collapse:collapse;width:100%;font-size:.86rem}
th,td{padding:7px 10px;border-bottom:1px solid var(--line);text-align:left;vertical-align:top}
th{background:var(--code);font-weight:600}
tbody tr:last-child td{border-bottom:0}
td code{white-space:normal;word-break:break-word}
.st{font-weight:600}
.banner{border:1px solid var(--line);background:var(--card);border-radius:12px;padding:14px 16px;margin:12px 0 18px}
.pills{display:flex;flex-wrap:wrap;gap:6px;margin-top:8px}
.pill{font-size:.75rem;font-weight:600;padding:2px 9px;border-radius:99px;border:1px solid var(--line)}
.pill.ok{color:var(--ok);background:var(--ok-bg)}.pill.warn{color:var(--warn);background:var(--warn-bg)}.pill.no{color:var(--no);background:var(--no-bg)}.pill.acc{color:var(--acc);background:var(--acc-bg)}
nav.toc{border:1px solid var(--line);border-radius:10px;background:var(--card);padding:10px 14px;margin:0 0 8px;font-size:.88rem}
nav.toc ol{columns:2;column-gap:28px;margin:4px 0;padding-left:1.2em}
@media (max-width:640px){nav.toc ol{columns:1}h1{font-size:1.35rem}}
.fig{margin:14px 0 20px;border:1px solid var(--line);border-radius:12px;background:var(--card);padding:10px}
.figscroll{overflow-x:auto}
.fig svg{display:block;width:100%;height:auto}
figcaption{font-size:.8rem;color:var(--muted);margin-top:6px}
.swipe{display:none}
@media (max-width:760px){.swipe{display:inline;font-weight:600;color:var(--acc)}}
/* svg tokens */
.zone{fill:var(--zone);stroke:var(--line)}
.frame{fill:var(--bg);stroke:var(--line)}
.bx{fill:var(--card);stroke:var(--muted);stroke-width:1.1}
.bx.acc{fill:var(--acc-bg);stroke:var(--acc)}.bx.ok{fill:var(--ok-bg);stroke:var(--ok)}.bx.warn{fill:var(--warn-bg);stroke:var(--warn)}.bx.no{fill:var(--no-bg);stroke:var(--no)}
.bx2{fill:var(--code);stroke:var(--line)}
.band.warn{fill:var(--warn-bg);stroke:var(--warn)}.band.no{fill:var(--no-bg);stroke:var(--no)}
.hdr{fill:var(--code);stroke:var(--line)}.pane{fill:var(--card);stroke:var(--line)}.spark{fill:var(--bg);stroke:var(--line)}
.chip{fill:var(--card);stroke:var(--line)}.chipok{fill:var(--ok-bg);stroke:var(--ok)}
.tab{fill:var(--code);stroke:var(--line)}.tabon{fill:var(--acc-bg);stroke:var(--acc)}
.btn{fill:var(--acc-bg);stroke:var(--acc)}.btnoff{fill:var(--code);stroke:var(--line)}
.grid{stroke:var(--line);stroke-width:1}
.line{fill:none;stroke:var(--acc);stroke-width:2}
.life{stroke:var(--line);stroke-dasharray:4 4}
.ar{stroke:var(--ink2);stroke-width:1.4;fill:none}.ar2{stroke:var(--acc);stroke-width:1.4}.arstop{stroke:var(--no);stroke-width:2}.arno{stroke:var(--no);stroke-width:1.4;stroke-dasharray:5 3}
.tun{fill:none;stroke:var(--warn);stroke-width:2.2;stroke-dasharray:7 4}
.ahf{fill:var(--ink2)}
.pillbg{fill:var(--card);stroke:var(--line)}
text{fill:var(--fg);font-family:-apple-system,BlinkMacSystemFont,"Apple SD Gothic Neo","Noto Sans KR",system-ui,sans-serif}
.t{font-size:13px}.tb{font-size:13.5px;font-weight:600}.th{font-size:14px;font-weight:700}.ts{font-size:11.5px;fill:var(--ink2)}.tsb{font-size:11.5px;font-weight:700;fill:var(--ok)}
.tm{font-size:12px;fill:var(--muted)}.tok{font-size:12px;fill:var(--ok);font-weight:600}.tw{font-size:12px;fill:var(--warn);font-weight:600}.tno{font-size:12px;fill:var(--no);font-weight:600}
.mono{font-size:11.5px;font-family:ui-monospace,SFMono-Regular,Menlo,monospace}
.foot{color:var(--muted);font-size:.8rem;margin-top:40px}
.benchlight{--bg:#f7f6f2;--fg:#1c1d1f;--muted:#5d5f63;--line:#d8d5cc;--card:#ffffff;--code:#efece4;--ok:#1d7448;--ok-bg:#e1f2e8;--no:#b2261e;--no-bg:#fbe6e3;--warn:#8a5900;--warn-bg:#fcefd4;--acc:#2a5db0;--acc-bg:#e4ecf8;--zone:#f1efe8;--ink2:#3a3c40;color-scheme:light}
.benchlight .figscroll{background:var(--bg);border-radius:8px}
.cpane{fill:#17191c;stroke:#2c3036}.ctext{fill:#e3e6ea;font-size:11.5px;font-family:ui-monospace,SFMono-Regular,Menlo,monospace}
.themebtn{float:right;font:inherit;font-size:.78rem;border:1px solid var(--line);background:var(--card);color:var(--fg);border-radius:99px;padding:2px 10px;cursor:pointer}
"""


def main():
    md = open(os.path.join(ROOT, "REPORT.md"), encoding="utf-8").read()
    body, toc = convert(md)
    toc_html = '<nav class="toc" aria-label="목차"><strong>목차</strong><ol>' + "".join(
        f'<li><a href="#{sid}">{html.escape(re.sub(r"^[0-9.]+ ", "", t))}</a></li>' for sid, t in toc) + "</ol></nav>"
    banner = ('<div class="banner"><button class="themebtn" type="button" onclick="(function(){var r=document.documentElement;r.dataset.theme=r.dataset.theme===\'dark\'?\'light\':\'dark\';try{localStorage.setItem(\'dbgReportTheme\',r.dataset.theme)}catch(e){}})()">라이트/다크</button><strong>상태:</strong> 구현 착수용 설계 보고서 — Loom에 dbg 도메인·브리지·패널은 아직 없음. '
              'CCS·프로브·하드웨어로 실행한 항목 없음. 코드 조각은 미실행.'
              '<div class="pills"><span class="pill acc">[문서] TI 공식 API</span><span class="pill ok">[제안] 이 보고서 설계</span>'
              '<span class="pill warn">[미확인] SDK·HW 확인 필요</span><span class="pill no">macOS 직접 구동 미지원</span></div></div>')
    # put banner + toc right after the h1
    body = body.replace("</h1>", "</h1>" + banner + toc_html, 1)
    doc = f"""<!doctype html>
<html lang="ko" data-theme="light">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>XDS560v2 디버거 구현 설계</title>
<style>{CSS}</style>
<script>try{{var t=localStorage.getItem('dbgReportTheme');if(t==='dark')document.documentElement.dataset.theme='dark';}}catch(e){{}}</script>
</head>
<body><main class="wrap">
{body}
<p class="foot">REPORT.md에서 생성 (tools/build_html.py) · 계약: contracts/ · API 근거: API-EVIDENCE.md/json · 접근일 2026-11-14</p>
</main></body></html>
"""
    open(os.path.join(ROOT, "REPORT.html"), "w", encoding="utf-8").write(doc)
    print("wrote", os.path.join(ROOT, "REPORT.html"), len(doc), "bytes")


if __name__ == "__main__":
    main()
