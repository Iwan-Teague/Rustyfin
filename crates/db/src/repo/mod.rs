pub mod calendar;
pub mod channel_transcripts;
pub mod channels;
pub mod episodes;
pub mod idempotency;
pub mod items;
pub mod jobs;
pub mod libraries;
pub mod media_files;
pub mod playstate;
pub mod settings;
pub mod setup_session;
pub mod users;
pub mod watch_party;

pub(crate) fn dollar_placeholders(start: usize, count: usize) -> String {
    (start..start + count)
        .map(|idx| format!("${idx}"))
        .collect::<Vec<_>>()
        .join(", ")
}
