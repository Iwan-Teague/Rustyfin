pub mod corpus;
pub mod memory_eval;
pub mod planner_eval;
pub mod report;
pub mod retrieval_eval;
pub mod tasks_eval;

use std::path::Path;

use anyhow::{Result, bail};

pub use report::{EvalReport, EvalSuiteReport, print_summary, write_report_json};

pub async fn run_mode(mode: &str, fixtures_dir: &Path) -> Result<EvalReport> {
    run_mode_internal(mode, fixtures_dir, true).await
}

pub async fn run_mode_for_task(mode: &str, fixtures_dir: &Path) -> Result<EvalReport> {
    run_mode_internal(mode, fixtures_dir, false).await
}

async fn run_mode_internal(
    mode: &str,
    fixtures_dir: &Path,
    include_self_referential_task_eval: bool,
) -> Result<EvalReport> {
    let mut suites = Vec::<EvalSuiteReport>::new();

    match mode {
        "planner" => suites.push(planner_eval::run(fixtures_dir).await?),
        "retrieval" => suites.push(retrieval_eval::run(fixtures_dir)?),
        "memory" => suites.push(memory_eval::run(fixtures_dir)?),
        "tasks" => suites.push(
            tasks_eval::run_with_options(fixtures_dir, include_self_referential_task_eval).await?,
        ),
        "all" | "default" => {
            suites.push(planner_eval::run(fixtures_dir).await?);
            suites.push(retrieval_eval::run(fixtures_dir)?);
            suites.push(memory_eval::run(fixtures_dir)?);
            suites.push(
                tasks_eval::run_with_options(fixtures_dir, include_self_referential_task_eval)
                    .await?,
            );
        }
        other => bail!("unknown eval mode: {other}"),
    }

    Ok(EvalReport::new(suites))
}
