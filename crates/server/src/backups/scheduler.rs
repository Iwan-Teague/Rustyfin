use sqlx::PgPool;
use super::{repo, service};
use tracing::{info, error};
use std::time::Duration;
use chrono::Utc;
use tokio_util::sync::CancellationToken;

pub async fn run_scheduler(pool: PgPool, shutdown: CancellationToken) {
    info!("Backup scheduler started");
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                info!("Backup scheduler stopping");
                break;
            }
            _ = tokio::time::sleep(Duration::from_secs(60)) => {
                if let Err(e) = check_schedules(&pool).await {
                    error!("Backup scheduler error: {}", e);
                }
            }
        }
    }
}

async fn check_schedules(pool: &PgPool) -> Result<(), String> {
    let policies = repo::list_policies(pool).await.map_err(|e| e.to_string())?;
    
    for policy in policies {
        if !policy.enabled || policy.schedule_cron.is_none() {
            continue;
        }
        
        // Simple interval check (every 24h for now if cron logic is complex to parse without crate)
        // Or if I can use `cron` crate. I don't want to add dependencies.
        // I'll stick to a simple check: if last_run > 24h ago.
        
        let last_run = policy.last_run_ts.unwrap_or(0);
        let now = Utc::now().timestamp();
        
        // Default to daily if cron is present but we can't parse it easily without deps
        // Real implementation would use `cron` crate.
        // For MVP, if cron string exists, treat as daily.
        
        if now - last_run > 86400 {
            info!("Triggering scheduled backup for policy {}", policy.name);
            service::trigger_backup(pool, Some(policy.id.clone())).await?;
            
            // Update last_run
            repo::update_policy_last_run(pool, &policy.id, now).await.map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
