use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use rustfin_server::ai_eval_harness;

enum Command {
    Eval {
        mode: String,
        json_out: Option<PathBuf>,
        markdown_out: Option<PathBuf>,
    },
    Gate {
        mode: ai_eval_harness::EvalGateMode,
        artifacts_dir: PathBuf,
        run_config: ai_eval_harness::EvalRunConfig,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    match parse_command(std::env::args().skip(1))? {
        Command::Eval {
            mode,
            json_out,
            markdown_out,
        } => run_eval_command(&mode, json_out, markdown_out).await,
        Command::Gate {
            mode,
            artifacts_dir,
            run_config,
        } => run_gate_command(mode, artifacts_dir, run_config).await,
    }
}

fn parse_command<I>(mut args: I) -> Result<Command>
where
    I: Iterator<Item = String>,
{
    let first = args.next().unwrap_or_else(|| "all".to_string());
    if first == "gate" {
        parse_gate_command(args)
    } else {
        parse_eval_command(first, args)
    }
}

fn parse_eval_command<I>(mode: String, mut args: I) -> Result<Command>
where
    I: Iterator<Item = String>,
{
    let mut json_out: Option<PathBuf> = None;
    let mut markdown_out: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        if arg == "--json-out" {
            let path = args.next().context("missing path after --json-out")?;
            json_out = Some(PathBuf::from(path));
        } else if arg == "--markdown-out" {
            let path = args.next().context("missing path after --markdown-out")?;
            markdown_out = Some(PathBuf::from(path));
        } else {
            bail!("unexpected argument: {arg}");
        }
    }

    Ok(Command::Eval {
        mode,
        json_out,
        markdown_out,
    })
}

fn parse_gate_command<I>(mut args: I) -> Result<Command>
where
    I: Iterator<Item = String>,
{
    let mut mode = ai_eval_harness::EvalGateMode::Smoke;
    let mut artifacts_dir: Option<PathBuf> = None;
    let mut run_config = ai_eval_harness::EvalRunConfig::default();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--mode" => {
                let value = args.next().context("missing value after --mode")?;
                mode = parse_gate_mode(&value)?;
            }
            "--artifacts-dir" => {
                let value = args.next().context("missing path after --artifacts-dir")?;
                artifacts_dir = Some(PathBuf::from(value));
            }
            "--generated-at" => {
                run_config.generated_at =
                    Some(args.next().context("missing value after --generated-at")?);
            }
            "--run-id" => {
                run_config.run_id = Some(args.next().context("missing value after --run-id")?);
            }
            "--git-sha" => {
                run_config.git_sha = Some(args.next().context("missing value after --git-sha")?);
            }
            "--base-sha" => {
                run_config.base_sha = Some(args.next().context("missing value after --base-sha")?);
            }
            "--dataset-version" => {
                run_config.dataset_version = Some(
                    args.next()
                        .context("missing value after --dataset-version")?,
                );
            }
            "--judge-version" => {
                run_config.judge_version =
                    Some(args.next().context("missing value after --judge-version")?);
            }
            "--rubric-version" => {
                run_config.rubric_version = Some(
                    args.next()
                        .context("missing value after --rubric-version")?,
                );
            }
            "--model-id" => {
                run_config.model_id = Some(args.next().context("missing value after --model-id")?);
            }
            "--backend-kind" => {
                run_config.backend_kind =
                    Some(args.next().context("missing value after --backend-kind")?);
            }
            "--seed" => {
                let value = args.next().context("missing value after --seed")?;
                run_config.seed = Some(
                    value
                        .parse::<u64>()
                        .with_context(|| format!("invalid --seed value: {value}"))?,
                );
            }
            "--timezone" => {
                run_config.timezone = Some(args.next().context("missing value after --timezone")?);
            }
            "--locale" => {
                run_config.locale = Some(args.next().context("missing value after --locale")?);
            }
            other => bail!("unexpected gate argument: {other}"),
        }
    }

    let artifacts_dir = artifacts_dir.context("gate mode requires --artifacts-dir")?;
    Ok(Command::Gate {
        mode,
        artifacts_dir,
        run_config,
    })
}

fn parse_gate_mode(value: &str) -> Result<ai_eval_harness::EvalGateMode> {
    match value {
        "smoke" => Ok(ai_eval_harness::EvalGateMode::Smoke),
        "release" => Ok(ai_eval_harness::EvalGateMode::Release),
        other => bail!("unknown gate mode: {other}"),
    }
}

async fn run_eval_command(
    mode: &str,
    json_out: Option<PathBuf>,
    markdown_out: Option<PathBuf>,
) -> Result<()> {
    let fixtures_dir = ai_eval_harness::corpus::fixtures_dir();
    if matches!(mode, "compare" | "comparison") {
        let report = ai_eval_harness::run_comparison_mode(&fixtures_dir).await?;
        if let Some(path) = json_out.as_ref() {
            ai_eval_harness::write_comparison_report_json(path, &report)?;
            println!("wrote {}", path.display());
        }
        if let Some(path) = markdown_out.as_ref() {
            ai_eval_harness::write_comparison_report_markdown(path, &report)?;
            println!("wrote {}", path.display());
        }

        ai_eval_harness::print_comparison_summary(&report);
        if !report.overall_pass {
            bail!("one or more AI comparison cases failed their thresholds");
        }
    } else {
        let report = ai_eval_harness::run_mode(mode, &fixtures_dir).await?;
        if let Some(path) = json_out.as_ref() {
            ai_eval_harness::write_report_json(path, &report)?;
            println!("wrote {}", path.display());
        }
        if let Some(path) = markdown_out.as_ref() {
            ai_eval_harness::write_report_markdown(path, &report)?;
            println!("wrote {}", path.display());
        }

        ai_eval_harness::print_summary(&report);
        if !report.overall_pass {
            bail!("one or more AI eval suites failed their thresholds");
        }
    }
    Ok(())
}

async fn run_gate_command(
    mode: ai_eval_harness::EvalGateMode,
    artifacts_dir: PathBuf,
    run_config: ai_eval_harness::EvalRunConfig,
) -> Result<()> {
    let fixtures_dir = ai_eval_harness::corpus::fixtures_dir();
    let summary = ai_eval_harness::run_judge_gate(
        &fixtures_dir,
        ai_eval_harness::EvalGateConfig {
            mode,
            artifacts_dir,
            run_config,
        },
    )
    .await?;

    println!(
        "AI judge gate {}: {}",
        mode.as_str(),
        if summary.gate_pass { "PASS" } else { "FAIL" }
    );
    println!("summary json: {}", summary.artifacts.summary_json);
    println!("summary markdown: {}", summary.artifacts.summary_markdown);
    println!("pointwise json: {}", summary.artifacts.pointwise_json);
    println!(
        "pointwise markdown: {}",
        summary.artifacts.pointwise_markdown
    );

    if let Some(path) = summary.artifacts.replay_json.as_ref() {
        println!("replay json: {}", path);
    }
    if let Some(path) = summary.artifacts.replay_markdown.as_ref() {
        println!("replay markdown: {}", path);
    }
    if let Some(path) = summary.artifacts.comparison_json.as_ref() {
        println!("comparison json: {}", path);
    }
    if let Some(path) = summary.artifacts.comparison_markdown.as_ref() {
        println!("comparison markdown: {}", path);
    }

    if !summary.gate_pass {
        for reason in &summary.failure_reasons {
            eprintln!("gate failure: {reason}");
        }
        bail!("AI judge gate failed");
    }
    Ok(())
}
