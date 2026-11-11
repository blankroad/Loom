# UDS 계획 근거용 스파이크

제품 구현이 아닌 라이브러리 연결·타이밍 가능성 검증이다. 물리 장비를 열지 않고 `python-can virtual`과 `127.0.0.1` TCP만 사용한다. 부모 UDS Client는 시험 harness이며, 제품은 상주 UDS 관리자가 Client를 소유해야 한다. 라이브 뷰어·사용자 개입·M1 완료를 이 스파이크로 주장하지 않는다.

저장소 루트에서 재현:

```sh
python3 -m venv spikes/uds/.venv
spikes/uds/.venv/bin/python -m pip install -r spikes/uds/requirements.txt
spikes/uds/.venv/bin/python spikes/uds/roundtrip.py
spikes/uds/.venv/bin/python spikes/uds/tls_probe.py
```

전역 설치 금지. 측정 환경은 macOS27.0.1 ARM64/CPython3.14.1. Python3.11 및 Windows/Linux 시험은 아직 수행하지 않았다. 재실행은 `evidence/roundtrip.json`, `evidence/tls-default-probe.json`을 갱신한다. 결과 수치는 해당 환경/시점의 샘플이며 최악 지연 보장이 아니다.

`roundtrip.py`는 별도 CAN 데몬 프로세스 안에서 virtual Bus 두 개와 ISO-TP 양단을 열고 Pipe 기반 custom BaseConnection으로 VIN을100회 읽는다. NRC0x78 두 번 후 성공, CAN 무응답 P2 timeout, FF/CF/FC 관측을 확인한다. Pipe ping500회는 IPC만 분리 측정한다. python-can virtual bus는 같은 프로세스 내에서만 공유되므로 Bus 양단을 같은 자식 안에 둔다. 이 Pipe는 최종 JSON-RPC/WS가 아니다.

로컬 DoIP entity는 ephemeral TCP port에서 routing activation, diagnostic ACK, VIN 응답을 처리한다. ACK와 응답을 한 send에 넣어 stream coalescing도 확인한다. 마지막 요청은 ACK 뒤 응답을 생략한다. **정상 DoIP 왕복은 성공했으나 P2=50ms 설정의 무응답 반환은 약2초**였다. 실패를 숨기지 않고 결과의 `silent_response_timeout`에 기록한다.

`tls_probe.py`는 doipclient1.3.0의 `use_secure=True` 기본 경로가 `check_hostname requires server_hostname`로 handshake 전에 실패함을 재현한다. TLS 서버/인증서 검증 성공을 시험하는 코드가 아니다. 정상 TLS 지원은 hostname/SNI와 CA 검증을 갖춘 어댑터를 만든 뒤 별도 검증해야 한다.

초기 시험은 can-isotp2.0.7의 send 인자를 `timeout`으로 오인해 실패했다. 실제 시그니처에 맞춰 `send_timeout`으로 수정했다. 원문 최신 예제를 고정 release와 혼용하지 않는다.

관측 당시 전체 의존성:

```text
can-isotp==2.0.7
doipclient==1.3.0
packaging==26.3
python-can==4.6.1
typing_extensions==4.16.0
udsoncan==1.26.1
wrapt==1.17.3
```

`evidence/pypi.json`, `github.json`, `cdd-repos.json`은2026-11-09에 실제 조회한 metadata다. 기타 `*-sources.json`은 조사한1차 출처 URL/응답 hash/짧은 발췌를 보관하며, 외부 소스 전체를 제품에 포함하지 않는다. 실패한 URL은 error로 남긴다. 최종 조사 판단과15개 절 계획은 `docs/plan/uds/r1-sol.md`를 참조한다.
