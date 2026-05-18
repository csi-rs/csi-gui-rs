use esp_csi_egui::app::App;
use esp_csi_egui::data::DEFAULT_HISTORY_CAP;
use esp_csi_egui::rerun_sink::Transport;
use esp_csi_egui::shared::DEFAULT_BAUD;
use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cfg = match parse_args() {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("{msg}");
            print_usage();
            std::process::exit(2);
        }
    };

    if !cfg!(feature = "rerun") && !matches!(cfg.rerun, Transport::Off) {
        log::warn!(
            "--rerun={} ignored: built without --features rerun",
            cfg.rerun.label()
        );
    }

    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("esp-csi-egui")
            .with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };

    eframe::run_native(
        "esp-csi-egui",
        native,
        Box::new(move |cc| {
            Ok(Box::new(App::new(
                cc,
                cfg.baud,
                cfg.history_size,
                cfg.rerun,
            )))
        }),
    )
}

struct CliArgs {
    baud: u32,
    history_size: usize,
    rerun: Transport,
}

fn print_usage() {
    eprintln!("usage: esp-csi-egui [options]");
    eprintln!();
    eprintln!("options:");
    eprintln!("  --baud=N | -b N           serial baud rate (default {DEFAULT_BAUD})");
    eprintln!(
        "  --history-size=N          in-memory packet history capacity (default {DEFAULT_HISTORY_CAP})"
    );
    eprintln!("  --rerun=MODE              rerun sink: off (default), spawn, connect[:host:port],");
    eprintln!("                            or file:PATH. Requires --features rerun at build time.");
    eprintln!("  -h, --help                show this help");
}

fn parse_args() -> Result<CliArgs, String> {
    let mut args = std::env::args().skip(1);
    let mut baud: u32 = DEFAULT_BAUD;
    let mut history_size: usize = DEFAULT_HISTORY_CAP;
    let mut rerun = Transport::Off;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            "--baud" | "-b" => {
                let v = args.next().ok_or_else(|| format!("{arg} expects a value"))?;
                baud = v.parse().map_err(|_| format!("invalid baud: {v:?}"))?;
            }
            "--history-size" => {
                let v = args.next().ok_or_else(|| format!("{arg} expects a value"))?;
                history_size = v
                    .parse()
                    .map_err(|_| format!("invalid history size: {v:?}"))?;
            }
            "--rerun" => {
                let v = args.next().ok_or_else(|| format!("{arg} expects a value"))?;
                rerun = parse_rerun_mode(&v)?;
            }
            s if s.starts_with("--baud=") => {
                let v = &s["--baud=".len()..];
                baud = v.parse().map_err(|_| format!("invalid baud: {v:?}"))?;
            }
            s if s.starts_with("-b=") => {
                let v = &s["-b=".len()..];
                baud = v.parse().map_err(|_| format!("invalid baud: {v:?}"))?;
            }
            s if s.starts_with("--history-size=") => {
                let v = &s["--history-size=".len()..];
                history_size = v
                    .parse()
                    .map_err(|_| format!("invalid history size: {v:?}"))?;
            }
            s if s.starts_with("--rerun=") => {
                let v = &s["--rerun=".len()..];
                rerun = parse_rerun_mode(v)?;
            }
            other => return Err(format!("unknown argument: {other:?}")),
        }
    }
    if history_size == 0 {
        return Err("--history-size must be >= 1".into());
    }
    Ok(CliArgs {
        baud,
        history_size,
        rerun,
    })
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
