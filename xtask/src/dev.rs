use crate::tasks::{ensure_demo_snapshot, ensure_wasm, repo_root, run, strip_inherited_cargo_env};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub struct DevOpts {
    pub demo: bool,
    pub regions: Vec<String>,
    pub gcp_projects: Vec<String>,
    pub azure_subscriptions: Vec<String>,
    pub cloudflare: bool,
    pub port: u16,
    pub poll_secs: u64,
    pub web_port: u16,
    pub skip_wasm: bool,
}

const READY_TIMEOUT: Duration = Duration::from_secs(300);

pub fn dev(mut opts: DevOpts) -> Result<(), String> {
    let root = repo_root();

    if !opts.skip_wasm {
        ensure_wasm(false)?;
    }
    if opts.demo {
        ensure_demo_snapshot()?;
    }

    run(&root, "cargo", &["build", "-p", "atlas-server"])?;
    let server_bin = target_dir(&root).join("debug/atlas-server");

    let mut server_args: Vec<String> = vec![
        "--port".into(),
        opts.port.to_string(),
        "--poll-secs".into(),
        opts.poll_secs.to_string(),
    ];
    if opts.demo {
        server_args.push("--demo".into());
    } else {
        if !opts.regions.is_empty() {
            server_args.push("--regions".into());
            server_args.append(&mut opts.regions);
        }
        if !opts.gcp_projects.is_empty() {
            server_args.push("--gcp-projects".into());
            server_args.append(&mut opts.gcp_projects);
        }
        if !opts.azure_subscriptions.is_empty() {
            server_args.push("--azure-subscriptions".into());
            server_args.append(&mut opts.azure_subscriptions);
        }
        if opts.cloudflare {
            server_args.push("--cloudflare".into());
        }
    }

    println!("\n▶ atlas-server {}", server_args.join(" "));
    let mut server = spawn_prefixed(
        Command::new(&server_bin)
            .args(&server_args)
            .current_dir(&root),
        "server",
    )?;

    if let Err(e) = wait_ready(&mut server, opts.port) {
        let _ = server.kill();
        let _ = server.wait();
        return Err(e);
    }

    println!(
        "\n▶ bun run dev (web on http://localhost:{})",
        opts.web_port
    );
    let web = spawn_prefixed(
        Command::new("bun")
            .args(["run", "dev"])
            .env("PORT", opts.web_port.to_string())
            .current_dir(root.join("atlas-render/atlas-web")),
        "web",
    );
    let web = match web {
        Ok(w) => w,
        Err(e) => {
            let _ = server.kill();
            let _ = server.wait();
            return Err(e);
        }
    };

    println!(
        "\natlas dev stack is up — web http://localhost:{}  api http://localhost:{}  (Ctrl-C stops everything)\n",
        opts.web_port, opts.port
    );
    supervise(vec![("server", server), ("web", web)])
}

fn target_dir(root: &std::path::Path) -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"))
}

fn spawn_prefixed(cmd: &mut Command, prefix: &'static str) -> Result<Child, String> {
    let mut child = strip_inherited_cargo_env(cmd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to start {prefix}: {e}"))?;
    forward(child.stdout.take(), prefix);
    forward(child.stderr.take(), prefix);
    Ok(child)
}

fn forward(pipe: Option<impl Read + Send + 'static>, prefix: &'static str) {
    let Some(pipe) = pipe else { return };
    std::thread::spawn(move || {
        for line in BufReader::new(pipe).lines().map_while(Result::ok) {
            println!("[{prefix}] {line}");
        }
    });
}

fn wait_ready(server: &mut Child, port: u16) -> Result<(), String> {
    let started = Instant::now();
    println!("waiting for atlas-server on :{port} …");
    loop {
        if http_ok(port, "/snapshot.json") {
            return Ok(());
        }
        if let Some(status) = server
            .try_wait()
            .map_err(|e| format!("failed to poll server: {e}"))?
        {
            return Err(format!("atlas-server exited during startup: {status}"));
        }
        if started.elapsed() > READY_TIMEOUT {
            return Err(format!(
                "atlas-server did not become ready within {}s",
                READY_TIMEOUT.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

fn http_ok(port: u16, path: &str) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(500)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut status_line = [0u8; 64];
    match stream.read(&mut status_line) {
        Ok(n) => String::from_utf8_lossy(&status_line[..n]).contains(" 200 "),
        Err(_) => false,
    }
}

fn supervise(mut children: Vec<(&'static str, Child)>) -> Result<(), String> {
    let (name, status) = 'outer: loop {
        for (name, child) in &mut children {
            if let Ok(Some(status)) = child.try_wait() {
                break 'outer (*name, status);
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    };

    println!("\n{name} exited ({status}) — stopping the rest");
    for (_, child) in &mut children {
        let _ = child.kill();
        let _ = child.wait();
    }
    if status.success() {
        Ok(())
    } else {
        Err(format!("{name} exited with {status}"))
    }
}
