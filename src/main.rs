use csi_gui_rs::app::{App, AppOptions};
use csi_gui_rs::data::DEFAULT_HISTORY_CAP;
use csi_gui_rs::probe::{self, ProbeOptions};
use csi_gui_rs::protocol::config::LogMode;
use csi_gui_rs::rerun_sink::Transport;
use csi_gui_rs::shared::DEFAULT_BAUD;
use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = match parse_args(std::env::args().skip(1)) {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("error: {msg}\n");
            print_usage();
            std::process::exit(2);
        }
    };
    if let Some(dir) = cli.data_dir.clone() {
        csi_gui_rs::paths::set_data_dir_override(dir);
    }

    if let Some(p) = cli.probe {
        let seconds = p.seconds;
        let results = probe::run(&ProbeOptions {
            ports: cli.app.ports.clone(),
            auto_attach: cli.app.auto_attach || (cli.app.ports.is_empty() && cli.app.simulate == 0),
            simulate: cli.app.simulate,
            baud: cli.app.baud,
            ..p
        });
        let ok = probe::report(&results, seconds);
        std::process::exit(if ok { 0 } else { 1 });
    }

    if !cfg!(feature = "rerun") && !matches!(cli.app.rerun, Transport::Off) {
        log::warn!("--rerun={} ignored: built without --features rerun", cli.app.rerun.label());
    }

    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("csi-gui-rs {}", env!("CARGO_PKG_VERSION")))
            .with_app_id("csi-gui-rs")
            .with_inner_size([1360.0, 840.0])
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };
    let opts = cli.app;
    eframe::run_native(
        "csi-gui-rs",
        native,
        Box::new(move |cc| Ok(Box::new(App::new(cc, opts)))),
    )
}

struct Cli {
    app: AppOptions,
    data_dir: Option<PathBuf>,
    probe: Option<ProbeOptions>,
}

fn print_usage() {
    eprintln!(
        "csi-gui-rs {} — live Wi-Fi CSI from ESP32 boards running esp-csi-cli-rs

usage:
  csi-gui-rs [options] [PORT...]          open the GUI
  csi-gui-rs probe [options] [PORT...]    headless check: identify, collect, report

options:
  PORT, -p PORT, --port=PORT   serial port to open (repeatable); e.g. /dev/ttyACM0, COM3
  -a, --auto-attach            open every ESP32-looking port, now and as they appear
  --simulate=N                 add N simulated esp-csi-cli-rs devices (no hardware needed)
  --start-all                  start every device as soon as it is ready
  -b N, --baud=N               baud for UART-bridged boards (default {DEFAULT_BAUD})
  --history-size=N             plot/replay window in 100 ms ticks (default {DEFAULT_HISTORY_CAP})
  --data-dir=DIR               where exports, recordings and .rrd files go
  --rerun=MODE                 off | spawn | connect[:host:port] | file:PATH (needs --features rerun)
  -V, --version                print the version
  -h, --help                   this help

probe options:
  --seconds=N                  how long to collect (default 5)
  --channel=N                  set every device to channel N
  --log-mode=MODE              serialized (default) | array-list | text | esp-csi-tool
  --esp-now-pair               pair the first two devices as ESP-NOW initiator/responder
  --record                     write per-device JSONL recordings to the data dir

With no PORT, probe attaches every ESP32 port. Exit status 0 when every device
identified and delivered CSI.",
        env!("CARGO_PKG_VERSION")
    );
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Cli, String> {
    let mut app = AppOptions::default();
    let mut data_dir = None;
    let mut probe: Option<ProbeOptions> = None;
    let mut args = args.peekable();
    if args.peek().map(String::as_str) == Some("probe") {
        args.next();
        probe = Some(ProbeOptions::default());
    }
    // `--flag=value` and `--flag value` both work.
    let mut args: Vec<String> = args.collect();
    args.reverse();
    while let Some(arg) = args.pop() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with('-') => (f.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        let mut value = |name: &str| -> Result<String, String> {
            inline.clone().or_else(|| args.pop()).ok_or_else(|| format!("{name} expects a value"))
        };
        match flag.as_str() {
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("csi-gui-rs {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "-p" | "--port" => app.ports.push(value(&flag)?),
            "-a" | "--auto-attach" => app.auto_attach = true,
            "--start-all" => app.start_all = true,
            "--simulate" => app.simulate = parse_num(&value(&flag)?, "--simulate")?,
            "-b" | "--baud" => app.baud = parse_num(&value(&flag)?, "--baud")?,
            "--history-size" => {
                app.history_size = parse_num(&value(&flag)?, "--history-size")?;
                if app.history_size == 0 {
                    return Err("--history-size must be >= 1".into());
                }
            }
            "--data-dir" => data_dir = Some(PathBuf::from(value(&flag)?)),
            "--rerun" => app.rerun = parse_rerun_mode(&value(&flag)?)?,
            "--seconds" | "--channel" | "--log-mode" | "--esp-now-pair" | "--record" => {
                let Some(p) = probe.as_mut() else {
                    return Err(format!("{flag} only applies to `csi-gui-rs probe`"));
                };
                match flag.as_str() {
                    "--seconds" => p.seconds = parse_num(&value(&flag)?, "--seconds")?,
                    "--channel" => p.channel = Some(parse_num(&value(&flag)?, "--channel")?),
                    "--log-mode" => {
                        let v = value(&flag)?;
                        p.log_mode = LogMode::ALL
                            .into_iter()
                            .find(|m| m.cli() == v)
                            .ok_or_else(|| format!("invalid --log-mode: {v:?}"))?;
                    }
                    "--esp-now-pair" => p.esp_now_pair = true,
                    _ => p.record = true,
                }
            }
            f if f.starts_with('-') => return Err(format!("unknown option: {f}")),
            port => app.ports.push(port.to_string()),
        }
    }
    Ok(Cli { app, data_dir, probe })
}

fn parse_num<T: std::str::FromStr>(v: &str, name: &str) -> Result<T, String> {
    v.trim().parse().map_err(|_| format!("invalid {name}: {v:?}"))
}

fn parse_rerun_mode(s: &str) -> Result<Transport, String> {
    if s == "off" {
        return Ok(Transport::Off);
    }
    if s == "spawn" {
        return Ok(Transport::Spawn);
    }
    if s == "connect" {
        return Ok(Transport::default_connect());
    }
    if let Some(rest) = s.strip_prefix("connect:") {
        let mut parts = rest.split(':');
        let host = parts.next().unwrap_or("127.0.0.1").to_string();
        let port: u16 = parts
            .next()
            .map(|p| p.parse::<u16>())
            .transpose()
            .map_err(|_| format!("invalid --rerun port in {s:?}"))?
            .unwrap_or(9876);
        return Ok(Transport::Connect { host, port });
    }
    if let Some(rest) = s.strip_prefix("file:") {
        if rest.is_empty() {
            return Err("--rerun=file: needs a path after the colon".into());
        }
        return Ok(Transport::File {
            path: PathBuf::from(rest),
        });
    }
    Err(format!("invalid --rerun mode: {s:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Cli {
        parse_args(s.split_whitespace().map(str::to_string)).unwrap()
    }

    #[test]
    fn ports_and_flags() {
        let c = parse("/dev/ttyACM0 -p /dev/ttyACM1 --port=COM3 -a --baud 921600 --simulate=2");
        assert_eq!(c.app.ports, ["/dev/ttyACM0", "/dev/ttyACM1", "COM3"]);
        assert!(c.app.auto_attach);
        assert_eq!(c.app.baud, 921_600);
        assert_eq!(c.app.simulate, 2);
        assert!(c.probe.is_none());
    }

    #[test]
    fn probe_subcommand() {
        let c = parse("probe --seconds=3 --channel 11 --log-mode=array-list --esp-now-pair /dev/ttyUSB0");
        let p = c.probe.unwrap();
        assert_eq!(p.seconds, 3);
        assert_eq!(p.channel, Some(11));
        assert_eq!(p.log_mode, LogMode::ArrayList);
        assert!(p.esp_now_pair);
        assert_eq!(c.app.ports, ["/dev/ttyUSB0"]);
    }

    #[test]
    fn probe_flags_need_probe() {
        assert!(parse_args(["--seconds=3".to_string()].into_iter()).is_err());
        assert!(parse_args(["--bogus".to_string()].into_iter()).is_err());
    }
}
