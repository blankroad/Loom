# SVG figures for REPORT.html (classes resolve to CSS tokens so light/dark both work)

def box(x, y, w, h, cls="bx", rx=8):
    return f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{rx}" class="{cls}"/>'

def txt(x, y, s, cls="t", anchor="start"):
    return f'<text x="{x}" y="{y}" class="{cls}" text-anchor="{anchor}">{s}</text>'

def pill(x, y, s, cls="ts", w=None):
    w = w or (len(s) * 6.6 + 14)
    return f'<rect x="{x - w / 2}" y="{y - 13}" width="{w}" height="18" rx="9" class="pillbg"/>' + txt(x, y, s, cls, "middle")

def arrow(x1, y1, x2, y2, cls="ar", label=None, lx=None, ly=None):
    s = f'<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" class="{cls}" marker-end="url(#ah)"/>'
    if label:
        s += txt(lx if lx is not None else (x1 + x2) / 2, ly if ly is not None else (y1 + y2) / 2 - 6, label, "ts", "middle")
    return s

DEFS = '<defs><marker id="ah" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" class="ahf"/></marker></defs>'

def svg(vb_w, vb_h, body, title, minw=680):
    return (f'<svg viewBox="0 0 {vb_w} {vb_h}" role="img" aria-label="{title}" style="min-width:{minw}px">'
            f'<title>{title}</title>{DEFS}{body}</svg>')

# ---------------------------------------------------------------- 1 deployment
def deployment():
    b = []
    # zones
    b.append(box(10, 30, 330, 300, "zone"))
    b.append(txt(24, 52, "macOS 워크스테이션 · Darwin arm64", "th"))
    b.append(box(360, 30, 340, 300, "zone"))
    b.append(txt(374, 52, "브리지 호스트 · x64 Windows / Linux", "th"))
    b.append(box(720, 30, 230, 300, "zone"))
    b.append(txt(734, 52, "하드웨어", "th"))
    # mac boxes
    b.append(box(26, 66, 140, 54, "bx"))
    b.append(txt(96, 88, "Loom UI", "tb", "middle")); b.append(txt(96, 106, "Bench Light 워크벤치", "ts", "middle"))
    b.append(box(184, 66, 140, 54, "bx"))
    b.append(txt(254, 88, "Luna · loom CLI", "tb", "middle")); b.append(txt(254, 106, "compact JSON", "ts", "middle"))
    b.append(box(26, 142, 298, 58, "bx acc"))
    b.append(txt(175, 164, "loomd gateway · dbg 도메인", "tb", "middle"))
    b.append(txt(175, 182, "위험표 · 정책(F1) · reducer · 저널 · 증거", "ts", "middle"))
    b.append(box(26, 222, 298, 92, "bx acc"))
    b.append(txt(175, 244, "dbg worker (프로브당 1)", "tb", "middle"))
    b.append(txt(175, 262, "직렬 큐 · watch 루프 · STOP latch", "ts", "middle"))
    b.append(box(40, 272, 120, 30, "bx2")); b.append(txt(100, 292, "FakeBackend", "ts", "middle"))
    b.append(box(190, 272, 120, 30, "bx2")); b.append(txt(250, 292, "CcsRemote", "ts", "middle"))
    b.append(arrow(96, 120, 120, 142)); b.append(arrow(254, 120, 230, 142))
    b.append(arrow(175, 200, 175, 222)); b.append(txt(183, 216, "loom-worker/1 (로컬 IPC)", "ts"))
    # tunnel
    b.append(f'<path d="M310,287 C345,287 345,120 384,120" class="tun" marker-end="url(#ah)"/>')
    b.append(pill(350, 330, "SSH -L 127.0.0.1:47560 · 토큰", "tw", 200))
    # bridge boxes
    b.append(box(384, 66, 300, 92, "bx warn"))
    b.append(txt(534, 88, "supervisor (Node, CCS 미로드)", "tb", "middle"))
    b.append(txt(534, 106, "127.0.0.1 bind · 토큰 · 클라이언트 1개", "ts", "middle"))
    b.append(txt(534, 124, "ctl 레인(ping/stop) 즉시 응답", "ts", "middle"))
    b.append(txt(534, 142, "큐 action›read›poll · 데드라인 · kill", "ts", "middle"))
    b.append(box(384, 180, 300, 56, "bx"))
    b.append(txt(534, 202, "executor (run.sh/run.bat)", "tb", "middle"))
    b.append(txt(534, 220, "CCS Scripting 동기 호출 1개씩", "ts", "middle"))
    b.append(arrow(534, 158, 534, 180, label="stdin/stdout JSON-lines", lx=612, ly=174))
    b.append(box(384, 254, 300, 60, "bx2"))
    b.append(txt(534, 276, "CCS 21.0.1 Debug Server · device support", "ts", "middle"))
    b.append(txt(534, 296, "Blackhawk 드라이버 (설치본 필요)", "ts", "middle"))
    b.append(arrow(534, 236, 534, 254))
    # hw
    b.append(box(740, 100, 190, 66, "bx"))
    b.append(txt(835, 124, "Blackhawk XDS560v2", "tb", "middle"))
    b.append(txt(835, 144, "USB 또는 LAN (모델 미정)", "ts", "middle"))
    b.append(box(740, 220, 190, 66, "bx"))
    b.append(txt(835, 244, "TI 타깃 코어", "tb", "middle"))
    b.append(txt(835, 264, "예: C28x (16비트 AU)", "ts", "middle"))
    b.append(arrow(684, 284, 740, 150)); b.append(pill(722, 240, "USB/LAN", "ts", 62))
    b.append(arrow(835, 166, 835, 220)); b.append(txt(845, 198, "JTAG / cJTAG", "ts"))
    b.append(txt(480, 362, "macOS 직접 560v2 구동: 공식 미지원 [S1][S2][S8] · npm/pip 래퍼만으로 Debug Server·드라이버를 대체할 수 없음 [A1–A3]", "ts", "middle"))
    return svg(960, 374, "".join(b), "배치도: macOS loomd에서 SSH 터널을 거쳐 브리지 호스트의 CCS Scripting과 XDS560v2로", 720)

# ---------------------------------------------------------------- 2 layout wireframe
def layout():
    b = []
    b.append(box(10, 10, 940, 500, "frame", 10))
    # header
    b.append(box(20, 20, 920, 46, "hdr", 6))
    chips = [("ccs-remote", 30, 92), ("bh1 · BH-XDS-560v2 LAN", 130, 168), ("bridge-win11 · 127.0.0.1:47560", 306, 200),
             ("CCS 21.0.1", 514, 80), ("cpu1 = &lt;core_name&gt;", 602, 140)]
    for s, x, w in chips:
        b.append(box(x, 31, w, 24, "chip", 12)); b.append(txt(x + w / 2, 48, s, "ts", "middle"))
    b.append(box(750, 31, 180, 24, "chipok", 12)); b.append(txt(840, 48, "RUNNING · 0.4 s 전 · e7/3", "tsb", "middle"))
    # left tree
    b.append(box(20, 76, 190, 350, "pane", 6))
    b.append(txt(32, 98, "타깃", "tb"))
    rows = [("▾ bh1 (link up · rtt 4.8 ms)", 120, "t"), ("  ● cpu1  RUNNING", 142, "tok"), ("  ○ cpu2  미연결", 164, "tm"),
            ("구성", 204, "tb"), ("target.ccxml  sha 9b1d…", 226, "ts"), ("app.out (심볼만)", 246, "ts"), ("source_map 1개", 266, "ts")]
    for s, y, c in rows:
        b.append(txt(32, y, s, c))
    # center tabs
    b.append(box(220, 76, 470, 350, "pane", 6))
    for i, (s, on) in enumerate([("Watch", True), ("Memory", False), ("Registers", False), ("Source·BP (이후)", False)]):
        x = 228 + i * 106
        b.append(box(x, 82, 100, 26, "tabon" if on else "tab", 5)); b.append(txt(x + 50, 100, s, "ts", "middle"))
    b.append(txt(232, 132, "key", "tm")); b.append(txt(330, 132, "값", "tm")); b.append(txt(450, 132, "sample_id", "tm")); b.append(txt(610, 132, "품질", "tm"))
    data = [("g_state", "0x0003 (3)", "bh1/cpu1@7.2.1", "stale", "tw"), ("g_cnt", "0x0001E240", "bh1/cpu1@7.2.1", "stale", "tw"),
            ("ADC_RESULT", "— would_halt", "gap", "refused", "tno"), ("PC (레지스터)", "마지막 halt e7/2", "회색 표시", "이전 epoch", "tm")]
    for i, (k, v, sid, q, c) in enumerate(data):
        y = 158 + i * 30
        b.append(f'<line x1="228" y1="{y+10}" x2="682" y2="{y+10}" class="grid"/>')
        b.append(txt(232, y, k, "t")); b.append(txt(330, y, v, "mono")); b.append(txt(450, y, sid, "mono")); b.append(txt(610, y, q, c))
    b.append(box(228, 290, 454, 86, "spark", 4))
    b.append('<polyline points="236,360 270,360 300,358 330,330 360,330 390,330" class="line"/>')
    b.append('<polyline points="430,330 470,330 520,312 560,312" class="line"/>')
    b.append(txt(410, 324, "gap", "tno", "middle"))
    b.append(txt(240, 304, "g_state · 요청 100 ms / 측정 p50 100.4 · p95 103.9", "ts"))
    b.append(box(228, 384, 72, 30, "btn", 5)); b.append(txt(264, 404, "Halt", "tb", "middle"))
    b.append(box(306, 384, 72, 30, "btnoff", 5)); b.append(txt(342, 404, "Run", "tm", "middle"))
    b.append(box(384, 384, 72, 30, "btnoff", 5)); b.append(txt(420, 404, "Step", "tm", "middle"))
    b.append(txt(470, 404, "operate 권한 · confirm none (F1)", "ts"))
    # right log
    b.append(box(700, 76, 240, 350, "pane", 6))
    b.append(txt(712, 98, "동작·오류 기록 (RowStore)", "tb"))
    logs = [("USR halt", "confirmed", "tok"), ("AGT watch w2", "matched", "tok"), ("USR run", "confirmed", "tok"),
            ("—  STOP", "큐 취소 0", "tw"), ("AGT halt", "E_STOPPED", "tno"), ("gap would_halt", "5 s · 50 poll", "tw")]
    for i, (a, s, c) in enumerate(logs):
        y = 124 + i * 28
        b.append(txt(712, y, a, "t")); b.append(txt(930, y, s, c, "end"))
    b.append(txt(712, 300, "증거: dbg:bh1/cpu1@seq:1045", "mono"))
    # bottom input
    b.append(box(20, 436, 920, 34, "cpane", 6))
    b.append(txt(32, 458, "› watch g_state --gt 3 --every 100ms   (초안 유지 · ↑↓ 히스토리 · 입력 포커스 중 단축키 비활성) — 선택형 다크 콘솔", "ctext"))
    b.append(txt(480, 494, "제안 와이어프레임 — 실제 프로브/CCS 화면이 아님 · 값은 예시", "tw", "middle"))
    return svg(960, 516, "".join(b), "Bench Light 디버거 워크벤치 레이아웃 제안 와이어프레임", 760)

# ---------------------------------------------------------------- 3 state machine
def states():
    b = []
    n = {"unknown": (80, 150), "connecting": (250, 150), "running": (450, 80), "halted": (450, 230),
         "stepping": (660, 230), "lost": (660, 80), "disconnected": (250, 300), "error": (860, 230)}
    cls = {"running": "bx ok", "halted": "bx acc", "lost": "bx no", "error": "bx no", "stepping": "bx warn"}
    for k, (x, y) in n.items():
        b.append(box(x - 62, y - 20, 124, 40, cls.get(k, "bx"), 20)); b.append(txt(x, y + 5, k, "tb", "middle"))
    b.append(arrow(142, 150, 186, 150)); b.append(txt(164, 128, "dbg.open", "ts", "middle"))
    b.append(arrow(312, 140, 386, 92)); b.append(txt(330, 104, "connect 관측", "ts", "end"))
    b.append(arrow(312, 160, 386, 218))
    b.append('<line x1="440" y1="100" x2="440" y2="208" class="ar" marker-end="url(#ah)"/><line x1="460" y1="210" x2="460" y2="102" class="ar" marker-end="url(#ah)"/>')
    b.append(txt(432, 160, "halt / BP hit", "ts", "end")); b.append(txt(468, 160, "run(false)", "ts"))
    b.append(arrow(512, 222, 596, 222)); b.append(txt(554, 214, "step", "ts", "middle"))
    b.append('<path d="M660,250 C660,292 450,292 450,252" class="ar" marker-end="url(#ah)"/>'); b.append(txt(555, 300, "step 완료 → halted", "ts", "middle"))
    b.append(arrow(512, 80, 596, 80)); b.append(txt(554, 108, "링크/executor 손실", "ts", "middle"))
    b.append('<path d="M660,60 C660,14 250,14 250,128" class="ar" marker-end="url(#ah)"/>')
    b.append(pill(455, 22, "명시적 dbg.open만 · 자동 재연결 없음", "tw", 250))
    b.append('<path d="M388,238 C330,260 300,270 290,280" class="ar" marker-end="url(#ah)"/>'); b.append(txt(350, 248, "dbg.close", "ts", "end"))
    b.append(arrow(722, 230, 796, 230)); b.append(txt(759, 222, "SDK 오류", "ts", "middle"))
    b.append(txt(860, 270, "(어느 상태에서든)", "tm", "middle"))
    b.append(txt(20, 350, "• run_epoch: running↔halted↔stepping 전이마다 +1 → 이전 epoch의 레지스터/메모리 값은 current=false(회색, 유지)", "ts"))
    b.append(txt(20, 370, "• state_rev: state·quality·link·session 변경마다 +1 → UI는 state_rev ≤ 현재인 이벤트 무시 · 초기값 unknown (가짜 halted/fresh 없음)", "ts"))
    b.append(txt(20, 390, "• quality: fresh / stale(age > stale_after_ms, lost 시 last_known 유지) / unknown(타임아웃·close 후 — close가 타깃 상태 유지를 보장하지 않음)", "ts"))
    return svg(940, 404, "".join(b), "타깃 run-state 상태 기계와 epoch/revision 규칙", 720)

# ---------------------------------------------------------------- 4 sequence
def sequence():
    b = []
    lanes = [("UI · Luna", 80), ("gateway·worker (mac)", 260), ("supervisor", 470), ("executor · CCS", 650), ("타깃", 830)]
    for s, x in lanes:
        b.append(box(x - 80, 14, 160, 30, "bx", 6)); b.append(txt(x, 34, s, "tb", "middle"))
        b.append(f'<line x1="{x}" y1="44" x2="{x}" y2="520" class="life"/>')
    def m(y, x1, x2, lab, cls="ar"):
        return arrow(x1 + (4 if x2 > x1 else -4), y, x2 - (4 if x2 > x1 else -4), y, cls, lab, (x1 + x2) / 2, y - 6)
    y = 72
    b.append(m(y, 80, 260, "watch.start (C0=dispatch cursor)")); y += 30
    b.append(m(y, 260, 470, "req read.batch prio=poll")); y += 26
    b.append(m(y, 470, 650, "stdin")); y += 26
    b.append(m(y, 650, 830, "isHalted() → memory.read()")); y += 26
    b.append(m(y, 830, 650, "", "ar2")); y += 4
    b.append(m(y + 20, 650, 260, "res done · sdk_call completed · effect none", "ar2")); y += 46
    b.append(m(y, 260, 80, "dbg.sample (sample_id) → 같은 레코드", "ar2")); y += 40
    b.append(box(190, y - 16, 560, 22, "band warn", 4)); b.append(txt(470, y, "action run(false) 진행 중 — executor 블록 (취소 불가)", "ts", "middle")); y += 30
    b.append(m(y, 80, 260, "STOP (Ctrl-] x)", "arstop")); y += 26
    b.append(m(y, 260, 470, "ctl stop", "arstop")); y += 26
    b.append(m(y, 470, 260, "ctl_res ≤ ms: cancelled[queued] · in_flight run", "ar2")); y += 28
    b.append(txt(470, y, "worker: watch 중지 · 대기 쓰기 failed(E_STOPPED) · halt/run/reset/disconnect 보내지 않음", "tw", "middle")); y += 28
    b.append(m(y, 650, 470, "res run done · effect applied (되돌리지 않음)", "ar2")); y += 30
    b.append(m(y, 80, 470, "agent target.halt → STOP_LATCHED (sdk_call not_started)", "arno")); y += 40
    b.append(box(190, y - 16, 560, 22, "band no", 4)); b.append(txt(470, y, "별도 세션: deadline + grace 초과 → executor kill → evt lost · effect unknown · 재시도 없음", "ts", "middle")); y += 30
    b.append(txt(470, y, "다음 요청 → NOT_OPEN (명시적 dbg.open 필요)", "tm", "middle"))
    return svg(920, 530, "".join(b), "watch, STOP, deadline 시퀀스: supervisor 제어 레인과 동기 executor", 760)

# ---------------------------------------------------------------- 5 tickets
def tickets():
    b = []
    N = {"T1": (70, 110, "모델·trait·Fake"), "T2": (230, 70, "worker·gateway"), "T3": (420, 26, "CLI"),
         "T4": (420, 92, "UI 패널 5"), "T4b": (230, 150, "Bench Light 토큰"), "T5": (420, 158, "스킬·Luna 하네스"),
         "T6": (230, 230, "supervisor+stub"), "T7": (420, 230, "CcsRemote"), "T8": (590, 230, "CCS 호스트 bring-up"),
         "T9": (760, 230, "하드웨어 수용"), "T10": (760, 120, "소스·BP·디스어셈블리"), "T11": (760, 320, "멀티코어·reset·write")}
    now = {"T1", "T2", "T3", "T4", "T4b", "T5", "T6", "T7"}
    for k, (x, y, s) in N.items():
        c = "bx ok" if k in now else ("bx warn" if k == "T8" else ("bx no" if k == "T9" else "bx"))
        b.append(box(x - 66, y - 22, 132, 44, c, 8)); b.append(txt(x, y - 4, k, "tb", "middle")); b.append(txt(x, y + 13, s, "ts", "middle"))
    E = [("T1", "T2"), ("T2", "T3"), ("T2", "T4"), ("T4b", "T4"), ("T2", "T5"), ("T1", "T6"), ("T6", "T7"), ("T2", "T7"), ("T7", "T8"), ("T8", "T9"), ("T9", "T10"), ("T9", "T11")]
    for a, c in E:
        x1, y1, _ = N[a]; x2, y2, _ = N[c]
        if x2 > x1:
            sx, sy, ex, ey = x1 + 66, y1, x2 - 66, y2
        elif y2 < y1:
            sx, sy, ex, ey = x1, y1 - 22, x2, y2 + 22
        else:
            sx, sy, ex, ey = x1, y1 + 22, x2, y2 - 22
        b.append(f'<line x1="{sx}" y1="{sy}" x2="{ex}" y2="{ey}" class="ar" marker-end="url(#ah)"/>')
    b.append(box(20, 360, 14, 14, "bx ok", 3)); b.append(txt(40, 372, "지금 구현 가능 (환경 불필요)", "ts"))
    b.append(box(240, 360, 14, 14, "bx warn", 3)); b.append(txt(260, 372, "CCS 호스트 필요", "ts"))
    b.append(box(400, 360, 14, 14, "bx no", 3)); b.append(txt(420, 372, "프로브·보드 필요", "ts"))
    b.append(box(560, 360, 14, 14, "bx", 3)); b.append(txt(580, 372, "T9 이후 (API 확인 결과에 따라)", "ts"))
    return svg(860, 386, "".join(b), "구현 티켓 의존 그래프", 700)
