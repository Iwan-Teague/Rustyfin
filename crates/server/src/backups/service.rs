use std::path::PathBuf;
use tokio::process::Command;
use tracing::{error, info};
use uuid::Uuid;
use sqlx::PgPool;
use crate::backups::repo::{self, BackupJob};

pub async fn trigger_backup(pool: &PgPool, policy_id: Option<String>) -> Result<String, String> {
    let job_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    
    let job = BackupJob {
        id: job_id.clone(),
        policy_id: policy_id.clone(),
        status: "pending".to_string(),
        trigger_type: if policy_id.is_some() { "scheduled".to_string() } else { "manual".to_string() },
        start_ts: now,
        end_ts: None,
        log_text: None,
        error_message: None,
        total_size_bytes: None,
    };

    repo::create_job(pool, &job).await.map_err(|e| e.to_string())?;

    let pool_clone = pool.clone();
    let job_id_clone = job_id.clone();
    
    tokio::spawn(async move {
        if let Err(e) = execute_backup(pool_clone, job_id_clone).await {
            error!("Backup failed: {}", e);
        }
    });

    Ok(job_id)
}

async fn execute_backup(pool: PgPool, job_id: String) -> Result<(), String> {
    let _start = chrono::Utc::now().timestamp();
    repo::update_job_status(&pool, &job_id, "running", None, None, None).await.map_err(|e| e.to_string())?;

    // Prepare backup directory
    let backup_root = std::env::var("RUSTFIN_BACKUP_DIR").unwrap_or_else(|_| ".backups".to_string());
    let backup_dir = PathBuf::from(backup_root).join(&job_id);
    
    tokio::fs::create_dir_all(&backup_dir).await.map_err(|e| e.to_string())?;

    // 1. Database Backup
    // Assuming pg_dump is available and DATABASE_URL is set
    let db_url = std::env::var("RUSTFIN_DATABASE_URL").map_err(|_| "RUSTFIN_DATABASE_URL not set".to_string())?;
    let db_dump_path = backup_dir.join("db.sql");
    
    let output = Command::new("pg_dump")
        .arg(&db_url)
        .arg("-f")
        .arg(&db_dump_path)
        .output()
        .await
        .map_err(|e| format!("Failed to execute pg_dump: {}", e))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).to_string();
        repo::update_job_status(&pool, &job_id, "failed", Some(chrono::Utc::now().timestamp()), Some(&err), None).await.map_err(|e| e.to_string())?;
        return Err(format!("pg_dump failed: {}", err));
    }

    // 2. Compress
    // For now, just calculating size of the folder or the single file
    let meta = tokio::fs::metadata(&db_dump_path).await.map_err(|e| e.to_string())?;
    let size = meta.len();

    // 3. Update Job
    let end = chrono::Utc::now().timestamp();
    repo::update_job_status(&pool, &job_id, "success", Some(end), None, Some(size as i64)).await.map_err(|e| e.to_string())?;
    
    info!("Backup job {} completed successfully", job_id);
    Ok(())
}

pub async fn restore_backup(pool: &PgPool, job_id: &str) -> Result<(), String> {
    // 1. Verify job exists and is successful
    let job = repo::get_job(pool, job_id).await.map_err(|e| e.to_string())?
        .ok_or_else(|| "Backup job not found".to_string())?;

    if job.status != "success" {
        return Err("Cannot restore from a failed or pending backup".to_string());
    }

    // 2. Locate artifact
    let backup_root = std::env::var("RUSTFIN_BACKUP_DIR").unwrap_or_else(|_| ".backups".to_string());
    let backup_dir = PathBuf::from(backup_root).join(job_id);
    let db_dump_path = backup_dir.join("db.sql");

    if !db_dump_path.exists() {
        return Err("Backup artifact not found on disk".to_string());
    }

    // 3. Spawn restore task (detached)
    // We spawn this detached because we might kill the server process during restore
    tokio::spawn(async move {
        if let Err(e) = execute_restore(db_dump_path).await {
            error!("Restore failed: {}", e);
        }
    });

    Ok(())
}

async fn execute_restore(dump_path: PathBuf) -> Result<(), String> {
    info!("Starting database restore from {:?}", dump_path);

    let db_url = std::env::var("RUSTFIN_DATABASE_URL").map_err(|_| "RUSTFIN_DATABASE_URL not set".to_string())?;
    
    // Parse DB name from URL to terminate other connections
    // Assuming format postgres://user:pass@host:port/dbname
    // Simple parsing logic or use a crate if available.
    // For now, rely on psql to handle it, or try to kill connections via psql.
    
    // Attempt to terminate other connections to this DB
    // We use a separate psql command to do this safely
    let terminate_cmd = "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE pid <> pg_backend_pid() AND datname = current_database();";
    
    let _ = Command::new("psql")
        .arg(&db_url)
        .arg("-c")
        .arg(terminate_cmd)
        .output()
        .await; // Ignore errors, best effort
        
    // Wait a moment for connections to close
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Run restore
    let output = Command::new("psql")
        .arg(&db_url)
        .arg("-f")
        .arg(&dump_path)
        .output()
        .await
        .map_err(|e| format!("Failed to execute psql restore: {}", e))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).to_string();
        error!("Restore psql failed: {}", err);
        return Err(format!("Restore failed: {}", err));
    }

    info!("Database restore completed successfully. Restarting server...");
    
    // Trigger shutdown/restart
    // In a systemd environment, exiting with success (0) or failure (1) usually triggers restart if Restart=always/on-failure
    // If we exit with 0, systemd might not restart if Restart=on-failure.
    // Ideally we want to signal "please restart me".
    // For now, exit 1 to force restart if on-failure, or just exit and hope supervisor handles it.
    std::process::exit(1);
}
