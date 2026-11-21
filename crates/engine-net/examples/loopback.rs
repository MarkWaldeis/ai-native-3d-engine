#[path = "../tests/support/mod.rs"]
mod support;
use anyhow::{bail, Result};
use engine_net::transport::LinkConfig;
use serde_json::json;

fn main() -> Result<()> {
    let mut rtt = 120.0;
    let mut jitter = 20.0;
    let mut loss = 0.05;
    let mut seconds = 10.0;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let value: f64 = args
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing value for {flag}"))?
            .parse()?;
        match flag.as_str() {
            "--rtt-ms" => rtt = value,
            "--jitter-ms" => jitter = value,
            "--loss" => loss = value,
            "--seconds" => seconds = value,
            _ => bail!("unknown argument {flag}"),
        }
    }
    if !rtt.is_finite()
        || rtt < 0.0
        || !jitter.is_finite()
        || jitter < 0.0
        || !(0.0..=1.0).contains(&loss)
        || !seconds.is_finite()
        || !(0.0..=600.0).contains(&seconds)
    {
        bail!("invalid network arguments");
    }
    let link = LinkConfig {
        latency_ms: rtt / 2.0,
        jitter_ms: jitter,
        loss,
        ..LinkConfig::default()
    };
    let lead = ((rtt / 2.0 + jitter) / engine_net::TICK_MS).ceil() as u64 + 2;
    let mut run = support::Harness::new(link, lead.min(30))?;
    for _ in 0..(seconds * 60.0) as u64 {
        run.step((seconds * 36.0) as u64)?;
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "server":run.server.stats(), "clients":run.clients.iter().map(|c| c.stats()).collect::<Vec<_>>(),
            "snapshots_received":run.snapshots_received,"simulated_packets_dropped":run.network.dropped_packets()
        }))?
    );
    Ok(())
}
