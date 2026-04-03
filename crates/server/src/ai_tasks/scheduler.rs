use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

use tokio::sync::Semaphore;

use crate::state::AppState;

use super::executor::run_task;
use super::store::{AiTaskStore, DbAiTaskStore};

#[derive(Debug)]
struct AiTaskScheduler {
    semaphore: Arc<Semaphore>,
    active: Arc<tokio::sync::Mutex<HashSet<String>>>,
}

impl AiTaskScheduler {
    fn new() -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(task_concurrency_limit())),
            active: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
        }
    }

    fn enqueue(&self, state: AppState, task_id: String) {
        let semaphore = self.semaphore.clone();
        let active = self.active.clone();
        tokio::spawn(async move {
            {
                let mut guard = active.lock().await;
                if !guard.insert(task_id.clone()) {
                    return;
                }
            }

            let _permit = semaphore
                .acquire_owned()
                .await
                .expect("ai task scheduler permit");
            let result = run_task(state.clone(), task_id.clone()).await;

            {
                let mut guard = active.lock().await;
                guard.remove(&task_id);
            }

            if let Err(error) = result {
                tracing::warn!(task_id = %task_id, error = %error, "ai task execution failed");
            }
        });
    }
}

fn scheduler() -> &'static AiTaskScheduler {
    static SCHEDULER: OnceLock<AiTaskScheduler> = OnceLock::new();
    SCHEDULER.get_or_init(AiTaskScheduler::new)
}

fn task_concurrency_limit() -> usize {
    std::env::var("RUSTFIN_AI_TASK_CONCURRENCY")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(2)
}

pub fn enqueue_task(state: AppState, task_id: impl Into<String>) {
    scheduler().enqueue(state, task_id.into());
}

pub async fn recover_pending_tasks(state: AppState) -> Result<(), String> {
    let store = DbAiTaskStore::new(state.db.clone());
    let tasks = store.list_recoverable_tasks().await?;
    for task in tasks {
        enqueue_task(state.clone(), task.id);
    }
    Ok(())
}
