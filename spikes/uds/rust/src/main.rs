//! Planning spike: candidate API checks and a bounded DoIP loopback, not a UDS runner.
use std::{io, time::Instant};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_util::{bytes::BytesMut, codec::Decoder};

fn frame(kind: u16, payload: &[u8]) -> Vec<u8> {
    assert!(payload.len() <= 4096);
    let mut b = vec![0x02, 0xfd];
    b.extend_from_slice(&kind.to_be_bytes());
    b.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    b.extend_from_slice(payload);
    b
}

async fn recv(s: &mut TcpStream) -> io::Result<(u16, Vec<u8>)> {
    let mut h = [0; 8];
    s.read_exact(&mut h).await?;
    let n = u32::from_be_bytes(h[4..8].try_into().unwrap()) as usize;
    if h[0] ^ h[1] != 255 || n > 4096 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid header/size",
        ));
    }
    let mut p = vec![0; n];
    s.read_exact(&mut p).await?;
    Ok((u16::from_be_bytes([h[2], h[3]]), p))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let central_good = automotive_diag::doip::ActivationType::CentralSecurity as u8;
    let central_candidate = doip_definitions::payload::ActivationType::CentralSecurity as u8;
    assert_eq!(central_good, 0xe0);
    let diag = frame(
        0x8001,
        &[0x10, 0x01, 0x0e, 0x80, 0x62, 0xf1, 0x90, 1, 2, 3, 4],
    );
    let mut codec = doip_codec::DoipCodec {};
    let mut partial = BytesMut::from(&diag[..12]);
    assert!(codec.decode(&mut partial).unwrap().is_none());
    assert_eq!(partial.len(), 12);
    partial.extend_from_slice(&diag[12..]);
    partial.extend_from_slice(&diag);
    assert!(codec.decode(&mut partial).unwrap().is_some());
    assert!(codec.decode(&mut partial).unwrap().is_some());
    assert!(partial.is_empty());
    let mut central = BytesMut::from(frame(0x0005, &[0x0e, 0x80, 0xe0, 0, 0, 0, 0]).as_slice());
    let central_rejected = codec.decode(&mut central).is_err();
    let mut oversize = BytesMut::from(&[2, 253, 128, 1, 255, 255, 255, 255][..]);
    let oversize_waits = matches!(codec.decode(&mut oversize), Ok(None));

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(async move {
        let (mut s, _) = listener.accept().await?;
        s.set_nodelay(true)?;
        loop {
            let (kind, p) = match recv(&mut s).await {
                Ok(v) => v,
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e),
            };
            if kind == 0x0005 {
                s.write_all(&frame(0x0006, &[0x0e, 0x80, 0x10, 0, 0x10, 0, 0, 0, 0]))
                    .await?;
            } else {
                assert_eq!(&p, &[0x0e, 0x80, 0x10, 0x01, 0x22, 0xf1, 0x90]);
                let mut response = vec![0x10, 0x01, 0x0e, 0x80, 0x62, 0xf1, 0x90];
                response.extend_from_slice(b"LOOMSIM0000000001");
                let mut both = frame(0x8002, &[0x10, 0x01, 0x0e, 0x80, 0]);
                both.extend_from_slice(&frame(0x8001, &response));
                s.write_all(&both).await?;
            }
        }
        Ok::<_, io::Error>(())
    });
    let mut client = TcpStream::connect(addr).await?;
    client.set_nodelay(true)?;
    client
        .write_all(&frame(0x0005, &[0x0e, 0x80, 0, 0, 0, 0, 0]))
        .await?;
    assert_eq!(recv(&mut client).await?.0, 0x0006);
    let request = frame(0x8001, &[0x0e, 0x80, 0x10, 0x01, 0x22, 0xf1, 0x90]);
    let mut times = Vec::with_capacity(1000);
    for i in 0..1020 {
        let start = Instant::now();
        client.write_all(&request).await?;
        let result = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            assert_eq!(recv(&mut client).await?.0, 0x8002);
            let (kind, p) = recv(&mut client).await?;
            assert_eq!(kind, 0x8001);
            assert_eq!(&p[7..], b"LOOMSIM0000000001");
            Ok::<_, io::Error>(())
        })
        .await??;
        let _ = result;
        if i >= 20 {
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }
    }
    drop(client);
    server.await??;
    times.sort_by(f64::total_cmp);
    println!(
        "{{\"schema_version\":1,\"platform\":\"{}-{}\",\"samples\":1000,\"warmup\":20,\"transport\":\"tokio current_thread loopback, no IPC/UI/TLS\",\"rtt_ms\":{{\"p50\":{},\"p99\":{},\"max\":{}}},\"candidate_checks\":{{\"fragment_and_coalescing\":true,\"automotive_diag_central_security\":{},\"doip_definitions_central_security\":{},\"doip_codec_rejects_e0\":{},\"doip_codec_oversize_header_returns_none\":{}}}}}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        times[499],
        times[989],
        times[999],
        central_good,
        central_candidate,
        central_rejected,
        oversize_waits
    );
    Ok(())
}
