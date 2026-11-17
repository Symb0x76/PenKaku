//! HTTP API of the Grbl_ESP32 web server on the wireless board.

use std::path::Path;
use std::time::Duration;

use reqwest::blocking::{Client, multipart};

use chixie_rs_core::Result;

/// SD folder the old app uses for tasks.
pub const TASK_DIR: &str = "/chixie_task";

pub struct Board {
    base: String,
    http: Client,
}

impl Board {
    pub fn new(ip: &str) -> Result<Self> {
        // The board is on the LAN: never route it through a system proxy.
        let http = Client::builder().timeout(Duration::from_secs(10)).no_proxy().build()?;
        Ok(Self { base: format!("http://{ip}"), http })
    }

    /// `GET /command?commandText=<cmd>&PAGEID=0`
    pub fn command(&self, cmd: &str) -> Result<String> {
        let url = format!("{}/command", self.base);
        Ok(self
            .http
            .get(url)
            .query(&[("commandText", cmd), ("PAGEID", "0")])
            .send()?
            .error_for_status()?
            .text()?)
    }

    pub fn status(&self) -> Result<String> { self.command("?") }
    pub fn hold(&self) -> Result<String> { self.command("!") }
    pub fn resume(&self) -> Result<String> { self.command("~") }
    pub fn unlock(&self) -> Result<String> { self.command("$X") }
    pub fn soft_reset(&self) -> Result<String> { self.command("\u{18}") }

    /// Run a G-code file stored on the SD card.
    pub fn run_sd_file(&self, path: &str) -> Result<String> {
        self.command(&format!("[ESP220]{path}"))
    }

    /// `GET /upload?path=<dir>[&action=..&filename=..]` → raw JSON text.
    fn upload_get(&self, dir: &str, action: Option<(&str, &str)>) -> Result<String> {
        let mut q = vec![("path", dir)];
        if let Some((a, f)) = action {
            q.push(("action", a));
            q.push(("filename", f));
        }
        Ok(self
            .http
            .get(format!("{}/upload", self.base))
            .query(&q)
            .send()?
            .error_for_status()?
            .text()?)
    }

    pub fn list(&self, dir: &str) -> Result<String> { self.upload_get(dir, None) }
    pub fn delete(&self, dir: &str, name: &str) -> Result<String> { self.upload_get(dir, Some(("delete", name))) }
    pub fn delete_dir(&self, dir: &str, name: &str) -> Result<String> { self.upload_get(dir, Some(("deletedir", name))) }
    pub fn create_dir(&self, dir: &str, name: &str) -> Result<String> { self.upload_get(dir, Some(("createdir", name))) }

    /// Multipart upload, field `myfile[]`, file stored as `<dir>/<name>`.
    pub fn upload(&self, dir: &str, name: &str, data: Vec<u8>) -> Result<String> {
        let target = format!("{}/{}", dir.trim_end_matches('/'), name);
        let part = multipart::Part::bytes(data).file_name(target);
        let form = multipart::Form::new().part("myfile[]", part);
        Ok(self
            .http
            .post(format!("{}/upload", self.base))
            .query(&[("path", dir)])
            .multipart(form)
            .send()?
            .error_for_status()?
            .text()?)
    }

    pub fn upload_file(&self, dir: &str, local: &Path) -> Result<String> {
        let name = local.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.upload(dir, &name, std::fs::read(local)?)
    }

    /// 上传SD卡书写: store `program` as `TASK_DIR/<name>` and start it on the board.
    pub fn upload_and_run(&self, name: &str, program: &str) -> Result<()> {
        self.upload(TASK_DIR, name, program.as_bytes().to_vec())?;
        self.run_sd_file(&format!("{TASK_DIR}/{name}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    use super::*;

    /// Answer `n` HTTP requests with 200 and return each one's request line and body.
    fn serve(n: usize) -> (String, std::thread::JoinHandle<Vec<(String, String)>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        let h = std::thread::spawn(move || {
            (0..n)
                .map(|_| {
                    let (s, _) = l.accept().unwrap();
                    let mut r = BufReader::new(s);
                    let mut first = String::new();
                    r.read_line(&mut first).unwrap();
                    let mut len = 0;
                    loop {
                        let mut h = String::new();
                        r.read_line(&mut h).unwrap();
                        if h.trim().is_empty() {
                            break;
                        }
                        if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                            len = v.trim().parse().unwrap();
                        }
                    }
                    let mut body = vec![0; len];
                    r.read_exact(&mut body).unwrap();
                    r.get_mut().write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").unwrap();
                    (first.trim().to_string(), String::from_utf8_lossy(&body).into_owned())
                })
                .collect()
        });
        (addr, h)
    }

    #[test]
    fn upload_then_run_from_the_task_folder() {
        let (addr, server) = serve(2);
        Board::new(&addr).unwrap().upload_and_run("p1.gcode", "G21\nG90\n").unwrap();
        let reqs = server.join().unwrap();
        assert!(reqs[0].0.starts_with("POST /upload?path=%2Fchixie_task "), "{}", reqs[0].0);
        assert!(reqs[0].1.contains("name=\"myfile[]\"") && reqs[0].1.contains("filename=\"/chixie_task/p1.gcode\""));
        assert!(reqs[0].1.contains("G21\nG90\n"));
        assert!(reqs[1].0.starts_with("GET /command?commandText=%5BESP220%5D%2Fchixie_task%2Fp1.gcode&PAGEID=0 "), "{}", reqs[1].0);
    }
}
