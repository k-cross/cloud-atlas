mod dev;
mod tasks;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[clap(about = "Cloud Atlas workspace orchestration", long_about = None)]
struct Opt {
    #[clap(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Dev {
        #[clap(long)]
        demo: bool,

        #[clap(long, num_args = 1..)]
        regions: Vec<String>,

        #[clap(long, num_args = 1..)]
        gcp_projects: Vec<String>,

        #[clap(long, num_args = 1..)]
        azure_subscriptions: Vec<String>,

        #[clap(long)]
        cloudflare: bool,

        #[clap(long, default_value_t = 4681)]
        port: u16,

        #[clap(long, default_value_t = 60)]
        poll_secs: u64,

        #[clap(long, default_value_t = 4680)]
        web_port: u16,

        #[clap(long)]
        skip_wasm: bool,
    },

    Wasm {
        #[clap(long)]
        force: bool,
    },
    Demo,

    Test {
        #[clap(long)]
        e2e: bool,
    },
}

fn main() {
    let result = match Opt::parse().command {
        Cmd::Dev {
            demo,
            regions,
            gcp_projects,
            azure_subscriptions,
            cloudflare,
            port,
            poll_secs,
            web_port,
            skip_wasm,
        } => dev::dev(dev::DevOpts {
            demo,
            regions,
            gcp_projects,
            azure_subscriptions,
            cloudflare,
            port,
            poll_secs,
            web_port,
            skip_wasm,
        }),
        Cmd::Wasm { force } => tasks::ensure_wasm(force),
        Cmd::Demo => tasks::demo_snapshot(),
        Cmd::Test { e2e } => tasks::test(e2e),
    };

    if let Err(e) = result {
        eprintln!("\nerror: {e}");
        std::process::exit(1);
    }
}
