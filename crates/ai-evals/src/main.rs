use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use rustfin_server::ai_eval_harness;

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_else(|| "all".to_string());
    let mut json_out: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        if arg == "--json-out" {
            let path = args.next().context("missing path after --json-out")?;
            json_out = Some(PathBuf::from(path));
        } else {
            bail!("unexpected argument: {arg}");
        }
    }

    let fixtures_dir = ai_eval_harness::corpus::fixtures_dir();
    let report = ai_eval_harness::run_mode(&mode, &fixtures_dir).await?;
    if let Some(path) = json_out.as_ref() {
        ai_eval_harness::write_report_json(path, &report)?;
        println!("wrote {}", path.display());
    }

    ai_eval_harness::print_summary(&report);
    if !report.overall_pass {
        bail!("one or more AI eval suites failed their thresholds");
    }
    Ok(())
}
