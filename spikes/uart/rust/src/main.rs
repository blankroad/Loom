//! Feasibility probe only: no Loom implementation and no physical UART claims.
use std::{fs::{File, OpenOptions}, io::{self, Read, Write}, time::{Duration, Instant}};
use serialport::SerialPort;

fn transfer_write(file: &mut File, data: &[u8]) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut done = 0;
    while done < data.len() {
        match file.write(&data[done..]) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(n) => done += n,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline { return Err(io::ErrorKind::TimedOut.into()); }
                std::thread::sleep(Duration::from_micros(100));
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn master_read(file: &mut File, count: usize) -> io::Result<Vec<u8>> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut data = vec![0; count];
    let mut done = 0;
    while done < count {
        match file.read(&mut data[done..]) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => done += n,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline { return Err(io::ErrorKind::TimedOut.into()); }
                std::thread::sleep(Duration::from_micros(100));
            }
            Err(e) => return Err(e),
        }
    }
    Ok(data)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pty = nix::pty::openpty(None, None)?;
    let mut raw = nix::sys::termios::tcgetattr(&pty.slave)?;
    nix::sys::termios::cfmakeraw(&mut raw);
    nix::sys::termios::tcsetattr(&pty.slave, nix::sys::termios::SetArg::TCSANOW, &raw)?;
    let path = nix::unistd::ttyname(&pty.slave)?;
    drop(pty.slave);
    nix::fcntl::fcntl(&pty.master, nix::fcntl::FcntlArg::F_SETFL(nix::fcntl::OFlag::O_NONBLOCK))?;
    let mut master = File::from(pty.master);
    println!("pty_created={} raw_slave_configured=true", path.display());
    let normal = serialport::new(path.to_string_lossy(), 115200)
        .timeout(Duration::from_millis(200)).dtr_on_open(false).open_native();
    println!("open_115200={:?}", normal.as_ref().map(|_| ()));
    // macOS PTYs reject IOSSIOSPEED; zero skips that physical-UART-only ioctl.
    // This fallback is explicit and belongs only to a test PTY adapter.
    let mut port = match normal {
        Ok(port) => port,
        Err(_) if cfg!(target_os = "macos") => serialport::new(path.to_string_lossy(), 0)
            .timeout(Duration::from_millis(200)).dtr_on_open(false).open_native()?,
        Err(e) => return Err(e.into()),
    };
    println!("platform={} arch={} slave={} exclusive={}", std::env::consts::OS,
        std::env::consts::ARCH, path.display(), port.exclusive());
    println!("second_plain_open={:?}", OpenOptions::new().read(true).write(true).open(&path).map(|_| ()));
    println!("dtr_false={:?} rts_false={:?}", port.write_data_terminal_ready(false), port.write_request_to_send(false));
    println!("set_break={:?} clear_break={:?}", port.set_break(), port.clear_break());

    let bytes: Vec<u8> = (0..=255).collect();
    transfer_write(&mut master, &bytes)?;
    let mut rx = vec![0; 256]; port.read_exact(&mut rx)?;
    assert_eq!(rx, bytes);
    port.write_all(&bytes)?;
    assert_eq!(master_read(&mut master, 256)?, bytes);
    println!("roundtrip_00_ff=256_each_direction_exact");

    let started = Instant::now();
    let mut one = [0u8; 1];
    println!("empty_read={:?} elapsed_ms={:.3}", port.read(&mut one), started.elapsed().as_secs_f64()*1000.0);

    let total = 4 * 1024 * 1024;
    let started = Instant::now();
    let writer = std::thread::spawn(move || -> io::Result<File> {
        let chunk: Vec<u8> = (0..65536).map(|n| n as u8).collect();
        for _ in 0..(total/chunk.len()) { transfer_write(&mut master, &chunk)?; }
        // Keep the master alive in the join result while the slave drains.
        Ok(master)
    });
    let mut done = 0;
    let mut hash = 0xcbf29ce484222325u64;
    let mut buffer = [0; 16384];
    while done < total {
        let n = port.read(&mut buffer)?;
        for (i, &byte) in buffer[..n].iter().enumerate() {
            assert_eq!(byte, ((done+i)%256) as u8);
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
        done += n;
    }
    let _master = writer.join().expect("writer panicked")?;
    let seconds = started.elapsed().as_secs_f64();
    println!("pty_rx_bytes={done} fnv1a64={hash:016x} elapsed_ms={:.3} mib_per_s={:.3}", seconds*1000.0, (total as f64/1048576.0)/seconds);
    port.set_exclusive(false)?;
    println!("after_set_exclusive_false_plain_open={:?}", OpenOptions::new().read(true).write(true).open(&path).map(|_| ()));

    for pattern in ["^(a+)+$", "^(a|aa)+$"] {
        let started = Instant::now();
        let re = regex::bytes::RegexBuilder::new(pattern)
            .size_limit(256*1024).dfa_size_limit(256*1024).build()?;
        println!("regex_compile pattern={pattern:?} us={:.3}", started.elapsed().as_secs_f64()*1e6);
        for size in [1024, 16384, 65536] {
            let mut haystack = vec![b'a'; size-1]; haystack.push(b'!');
            let started = Instant::now();
            for _ in 0..100 { assert!(re.find(std::hint::black_box(&haystack)).is_none()); }
            println!("regex_find pattern={pattern:?} bytes={size} iterations=100 mean_us={:.3}", started.elapsed().as_secs_f64()*1e6/100.0);
        }
    }
    println!("unsupported_backref={:?}", regex::bytes::Regex::new(r"(a)\1").map(|_| ()));
    let huge = regex::bytes::RegexBuilder::new(r"\w{1000}").size_limit(256*1024).build();
    println!("unicode_expansion_rejected={}", huge.is_err());
    assert!(huge.is_err());
    assert_eq!(memchr::memmem::find(b"abc\0def", b"\0d"), Some(3));
    Ok(())
}
