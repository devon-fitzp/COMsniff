use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::config::{ConfigSettings, Encoding};

/// Read timeout used by both forwarder threads. Bounds how long
/// `SerialSession::stop` can take to join (the thread rechecks the stop flag
/// every time a read times out), so this must stay well below anything a
/// human would perceive as UI lag.
const READ_TIMEOUT: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortSide {
    Left,
    Right,
}

/// Sent from a forwarder thread to the main loop. Carries raw bytes, not
/// decoded text -- decoding needs `ConfigSettings::encoding`, which lives on
/// the main thread, so both `log_lines` and the log file decode identically.
pub enum SerialEvent {
    Chunk { side: PortSide, bytes: Vec<u8> },
    Error { side: PortSide, message: String },
}

/// Owns everything a successful Start creates: both forwarder threads, the
/// shared stop flag, and the receiving end of the channel. `stop` is the only
/// way threads and ports get cleaned up -- dropping this without calling it
/// would abandon the threads (they'd still exit on the next read timeout once
/// nothing is polling the channel, but ports wouldn't close until then).
pub struct SerialSession {
    stop: Arc<AtomicBool>,
    handle_left: JoinHandle<()>,
    handle_right: JoinHandle<()>,
    pub rx: Receiver<SerialEvent>,
}

impl SerialSession {
    /// Opens both ports with `config` and spawns the two forwarder threads.
    /// All fallible setup (open, try_clone) happens here, synchronously,
    /// before any thread is spawned -- so a failure here never leaves an
    /// orphaned thread to clean up; any already-opened port just drops.
    pub fn start(left_name: &str, right_name: &str, config: &ConfigSettings) -> io::Result<SerialSession> {
        let left = open_port(left_name, config)?;
        let right = open_port(right_name, config)?;

        let left_writer = left.try_clone()?;
        let right_writer = right.try_clone()?;

        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();

        let stop_left = Arc::clone(&stop);
        let tx_left = tx.clone();
        let handle_left =
            std::thread::spawn(move || forward_loop(PortSide::Left, left, right_writer, stop_left, tx_left));

        let stop_right = Arc::clone(&stop);
        let handle_right =
            std::thread::spawn(move || forward_loop(PortSide::Right, right, left_writer, stop_right, tx));

        Ok(SerialSession { stop, handle_left, handle_right, rx })
    }

    /// Signals both threads to stop and joins them. Bounded by
    /// `READ_TIMEOUT` since that's the longest either thread can be blocked
    /// in `read()` before it rechecks the flag. Ports close when the
    /// threads' owned port/writer handles drop at thread exit.
    pub fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.handle_left.join();
        let _ = self.handle_right.join();
    }
}

fn open_port(name: &str, config: &ConfigSettings) -> io::Result<Box<dyn serialport::SerialPort>> {
    serialport::new(name, config.baud_rate)
        .data_bits(config.data_bits)
        .stop_bits(config.stop_bits)
        .parity(config.parity)
        .timeout(READ_TIMEOUT)
        .open()
        .map_err(io::Error::from)
}

/// Reads from `reader`, writes everything read to `writer` (the other port),
/// and sends a `Chunk`/`Error` event per read. `TimedOut`/`Interrupted` are
/// the expected result of an idle port with a short read timeout and must
/// `continue`, not report failure -- treating them as errors would auto-stop
/// the session the instant either port went quiet.
fn forward_loop(
    side: PortSide,
    mut reader: Box<dyn serialport::SerialPort>,
    mut writer: Box<dyn serialport::SerialPort>,
    stop: Arc<AtomicBool>,
    tx: Sender<SerialEvent>,
) {
    let mut buf = [0u8; 1024];
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        match reader.read(&mut buf) {
            Ok(0) => continue,
            Ok(n) => {
                let bytes = buf[..n].to_vec();
                if writer.write_all(&bytes).is_err() {
                    let _ = tx.send(SerialEvent::Error { side, message: "write to peer port failed".into() });
                    return;
                }
                let _ = tx.send(SerialEvent::Chunk { side, bytes });
            }
            Err(e) if e.kind() == io::ErrorKind::TimedOut || e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                let _ = tx.send(SerialEvent::Error { side, message: e.to_string() });
                return;
            }
        }
    }
}

/// Decodes a raw chunk per the user's chosen encoding. A chunk boundary that
/// splits a multibyte UTF-8 character produces a replacement character --
/// accepted, since the requirement is one `LogLine` per chunk, not full line
/// reassembly.
pub fn decode_chunk(encoding: Encoding, bytes: &[u8]) -> String {
    match encoding {
        Encoding::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
        Encoding::Ascii => bytes.iter().map(|&b| if (0x20..=0x7E).contains(&b) { b as char } else { '.' }).collect(),
    }
}
