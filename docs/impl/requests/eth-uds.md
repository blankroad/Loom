# eth → uds 요청 (2026-11-11, explore-lead)

**DoIP 진단 패킷 ↔ UDS 요청 링크(`eth.link`)를 위해**, UDS가 DoIP로 보낸 요청마다 `uds.tx` 행 payload에 다음 `wire_key`를 넣어 달라:
`wire_key = {transport:"doip", src_ip, dst_ip, src_port, dst_port, doip_sa, doip_ta, uds_sha256_8}`(UDS 바이트 sha256 앞 8바이트 hex).
- eth gateway가 같은 키의 `eth.pkt`(DoIP 0x8001/0x8002/0x8003)를 찾아 `eth.link{eth_ref, domain:"uds", request_id, role: request|ack|nack|response, confidence: exact|probable, target}`를 낸다. UI는 양방향 칩으로 보여 준다(`_xteam/eth-uds.md` 합의 내용).
- 응답 쪽(`uds.rx`)에도 같은 키(방향 반대)를 넣어 주면 response 링크도 exact가 된다.
- 그때까지 eth는 `eth.link`를 만들지 않는다(추측 링크 금지).
- 대상 이름(`target`)도 함께 주면 UI 칩에 표시한다.
