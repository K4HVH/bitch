use anyhow::{Context, Result};
use mlua::Lua;
use serialport::{FlowControl, SerialPort};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::Duration;
use tracing::{debug, info, warn};

// Serial write request
struct SerialWrite {
    baudrate: u32,
    data: Vec<u8>,
    timeout_ms: u64,
}

// Per-port queues (one worker thread per port)
static PORT_QUEUES: Mutex<Option<HashMap<String, Sender<SerialWrite>>>> = Mutex::new(None);

// Keep-alive timeout: close port after 500ms of idle time
const KEEPALIVE_TIMEOUT_MS: u64 = 500;

/// Initialize serial API for Lua
pub fn init(lua: &Lua) -> Result<()> {
    // Initialize the port queues map
    init_port_queues();

    let serial_table = lua.create_table()
        .map_err(|e| anyhow::anyhow!("Failed to create serial table: {}", e))?;

    // serial.write(port, baudrate, data, [timeout_ms])
    serial_table.set(
        "write",
        lua.create_function(|_, (port, baudrate, data, timeout): (String, u32, String, Option<u64>)| {
            let timeout_ms = timeout.unwrap_or(3000);

            // Queue the write request
            queue_serial_write(port, baudrate, data.into_bytes(), timeout_ms);

            // Return immediately without blocking
            Ok(true)
        }).map_err(|e| anyhow::anyhow!("Failed to create serial.write: {}", e))?,
    ).map_err(|e| anyhow::anyhow!("Failed to set serial.write: {}", e))?;

    // serial.write_line(port, baudrate, data, [timeout_ms])
    serial_table.set(
        "write_line",
        lua.create_function(|_, (port, baudrate, data, timeout): (String, u32, String, Option<u64>)| {
            let timeout_ms = timeout.unwrap_or(3000);
            let mut line_data = data;
            line_data.push('\n');

            // Queue the write request
            queue_serial_write(port, baudrate, line_data.into_bytes(), timeout_ms);

            // Return immediately without blocking
            Ok(true)
        }).map_err(|e| anyhow::anyhow!("Failed to create serial.write_line: {}", e))?,
    ).map_err(|e| anyhow::anyhow!("Failed to set serial.write_line: {}", e))?;

    lua.globals().set("serial", serial_table)
        .map_err(|e| anyhow::anyhow!("Failed to set serial global: {}", e))?;

    Ok(())
}

/// Initialize the port queues map
fn init_port_queues() {
    let mut queues = PORT_QUEUES.lock().unwrap();
    if queues.is_none() {
        *queues = Some(HashMap::new());
    }
}

/// Get or create a queue for the specified port
fn get_or_create_queue(port: &str) -> Sender<SerialWrite> {
    let mut queues = PORT_QUEUES.lock().unwrap();

    if queues.is_none() {
        *queues = Some(HashMap::new());
    }

    if let Some(ref mut map) = *queues {
        // Return existing queue if available
        if let Some(sender) = map.get(port) {
            return sender.clone();
        }

        // Create new queue and worker thread for this port
        let (tx, rx) = channel::<SerialWrite>();
        let port_owned = port.to_string();

        std::thread::spawn(move || {
            serial_worker(port_owned, rx);
        });

        map.insert(port.to_string(), tx.clone());
        tx
    } else {
        unreachable!("PORT_QUEUES should be initialized");
    }
}

/// Worker thread for a specific serial port
/// Keeps port open while commands are queued, closes after idle timeout
fn serial_worker(port_path: String, rx: Receiver<SerialWrite>) {
    info!("[Serial] Worker thread started for {}", port_path);

    loop {
        // Wait for first command (blocking)
        let first_req = match rx.recv() {
            Ok(req) => req,
            Err(_) => {
                debug!("[Serial] Channel closed for {}", port_path);
                break;
            }
        };

        // Open the serial port
        let mut serial_port = match open_serial_port(&port_path, first_req.baudrate, first_req.timeout_ms) {
            Ok(port) => port,
            Err(e) => {
                warn!("[Plugin] Failed to open {}: {}", port_path, e);
                continue;
            }
        };

        info!("[Serial] Port {} opened, waiting for Arduino boot (2.5s)", port_path);

        // First command: wait for Arduino to boot after DTR reset
        std::thread::sleep(Duration::from_millis(2500));

        // Send first command
        if let Err(e) = send_data(&mut serial_port, &first_req.data) {
            warn!("[Plugin] Failed to write to {}: {}", port_path, e);
            continue;
        }

        debug!("[Plugin] Serial write to {} succeeded (first)", port_path);

        // Keep port open and process queued commands with keep-alive
        let mut command_count = 1;
        loop {
            match rx.recv_timeout(Duration::from_millis(KEEPALIVE_TIMEOUT_MS)) {
                Ok(req) => {
                    // More commands in queue! Send immediately (no delay)
                    if let Err(e) = send_data(&mut serial_port, &req.data) {
                        warn!("[Plugin] Failed to write to {}: {}", port_path, e);
                        break;
                    }
                    command_count += 1;
                    debug!("[Plugin] Serial write to {} succeeded (queued #{})", port_path, command_count);
                }
                Err(RecvTimeoutError::Timeout) => {
                    // No more commands after timeout, close port
                    info!("[Serial] Closing {} after {}ms idle ({} commands sent)",
                          port_path, KEEPALIVE_TIMEOUT_MS, command_count);
                    drop(serial_port);
                    break;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    // Channel closed, exit worker
                    info!("[Serial] Channel disconnected for {}", port_path);
                    drop(serial_port);
                    return;
                }
            }
        }
    }

    info!("[Serial] Worker thread ended for {}", port_path);
}

/// Open a serial port with the specified configuration
fn open_serial_port(port: &str, baudrate: u32, timeout_ms: u64) -> Result<Box<dyn SerialPort>> {
    serialport::new(port, baudrate)
        .timeout(Duration::from_millis(timeout_ms))
        .flow_control(FlowControl::None)
        .open()
        .with_context(|| format!("Failed to open serial port {}", port))
}

/// Send data to an open serial port
fn send_data(port: &mut Box<dyn SerialPort>, data: &[u8]) -> Result<()> {
    port.write_all(data)
        .context("Failed to write to serial port")?;

    port.flush()
        .context("Failed to flush serial port")?;

    Ok(())
}

/// Queue a serial write request for the specified port
fn queue_serial_write(port: String, baudrate: u32, data: Vec<u8>, timeout_ms: u64) {
    let sender = get_or_create_queue(&port);

    let req = SerialWrite {
        baudrate,
        data,
        timeout_ms,
    };

    // Send to queue (non-blocking)
    if let Err(e) = sender.send(req) {
        warn!("[Serial] Failed to queue write to {}: {}", port, e);
    }
}
