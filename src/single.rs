//! One Pulse at a time. Opening Pulse while it's already running brings the open
//! window to the front instead of starting a second player.
//!
//! The running Pulse listens on a port that only this computer can reach
//! (127.0.0.1, so no firewall prompt). A new Pulse knocks there first; if Pulse
//! answers, the new one asks it to show itself and quits.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Mutex;
use std::time::Duration;

const PORT: u16 = 47_613;
const KNOCK: &[u8] = b"pulse:show\n";
const ANSWER: &[u8] = b"pulse:ok\n";

static LISTENER: Mutex<Option<TcpListener>> = Mutex::new(None);

/// False when another Pulse is already running (it has been asked to show itself).
pub fn claim() -> bool {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, PORT));
    match TcpListener::bind(addr) {
        Ok(l) => {
            *LISTENER.lock().unwrap() = Some(l);
            true
        }
        // Something has the port: if it's Pulse, it answers.
        Err(_) => !knock(addr),
    }
}

fn knock(addr: SocketAddr) -> bool {
    let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_millis(400)) else { return false };
    let _ = s.set_read_timeout(Some(Duration::from_millis(800)));
    if s.write_all(KNOCK).is_err() {
        return false;
    }
    let mut reply = [0u8; ANSWER.len()];
    s.read_exact(&mut reply).is_ok() && reply == ANSWER
}

/// In the running Pulse: shows the window whenever another Pulse knocks.
pub fn listen(ctx: egui::Context) {
    let Some(listener) = LISTENER.lock().unwrap().take() else { return };
    std::thread::Builder::new()
        .name("single-instance".into())
        .spawn(move || {
            // Blocks waiting for a knock: no polling, no CPU while idle.
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                let mut buf = [0u8; KNOCK.len()];
                if stream.read_exact(&mut buf).is_ok() && buf == KNOCK {
                    let _ = stream.write_all(ANSWER);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    ctx.request_repaint();
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_pulse_is_told_to_step_aside() {
        // A stand-in for the running Pulse on a spare port.
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut buf = [0u8; KNOCK.len()];
            s.read_exact(&mut buf).unwrap();
            s.write_all(ANSWER).unwrap();
        });
        assert!(knock(addr));
    }

    #[test]
    fn something_else_on_the_port_isnt_mistaken_for_pulse() {
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let _ = s.write_all(b"HTTP/1.1 400 Bad\r\n");
        });
        assert!(!knock(addr));
    }
}
