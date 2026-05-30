//! Recursive file-system watcher for the LAN folder sync feature.
//!
//! Uses the [`notify`](https://docs.rs/notify/6) crate (v6) with a 100 ms
//! debounce window to coalesce rapid changes.  Each meaningful filesystem
//! event is translated into a [`SyncEvent`] and pushed through a
//! [`tokio::sync::mpsc::UnboundedSender`] so the sync server can react.
//!
//! All paths emitted are **relative** to the watched folder root.
//!
//! # Example
//!
//! ```rust,no_run
//! use tokio::sync::mpsc;
//! use std::path::PathBuf;
//!
//! #[tokio::main]
//! async fn main() {
//!     let (tx, mut rx) = mpsc::unbounded_channel();
//!     let root = PathBuf::from("./my_folder");
//!
//!     tokio::spawn(async move {
//!         filedrop::sync::watcher::watch_folder(root, tx)
//!             .await
//!             .expect("watcher failed");
//!     });
//!
//!     while let Some(ev) = rx.recv().await {
//!         println!("{:?}", ev);
//!     }
//! }
//! ```

use anyhow::{Context, Result};
use notify::{
    event::{CreateKind, ModifyKind, RemoveKind, RenameMode},
    Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
};
use std::path::PathBuf;
use std::sync::mpsc as std_mpsc;
use std::time::Duration;
use tokio::sync::mpsc;

// ── Public types ────────────────────────────────────────────────────────────

/// A single filesystem change expressed relative to the watched root.
#[derive(Debug, Clone)]
pub enum SyncEvent {
    /// A new regular file appeared.
    FileCreated { relative_path: PathBuf },
    /// An existing file's content was modified.
    FileModified { relative_path: PathBuf },
    /// A file was removed.
    FileDeleted { relative_path: PathBuf },
    /// A new directory was created.
    DirCreated { relative_path: PathBuf },
}

// ── Debounce window ─────────────────────────────────────────────────────────

/// Duration of the debounce window used to coalesce rapid FS events.
const DEBOUNCE_MS: u64 = 100;

// ── Public API ──────────────────────────────────────────────────────────────

/// Watch `root` recursively and emit [`SyncEvent`]s through `tx`.
///
/// This function blocks (via `spawn_blocking`) until:
/// * The sender half of `tx` is dropped / closed, **or**
/// * An unrecoverable watcher error occurs.
///
/// It is designed to be called inside a `tokio::spawn` task.
pub async fn watch_folder(
    root: PathBuf,
    tx: mpsc::UnboundedSender<SyncEvent>,
) -> Result<()> {
    // Canonicalise so we can safely strip the prefix later.
    let canonical_root = tokio::fs::canonicalize(&root)
        .await
        .with_context(|| format!("[SYNC] Cannot canonicalise root: {}", root.display()))?;

    tracing::info!(
        "[SYNC] Watching folder: {}",
        canonical_root.display()
    );

    // Notify v6 operates with std channels; bridge into tokio via
    // spawn_blocking so we never block the async runtime.
    let watch_root = canonical_root.clone();
    tokio::task::spawn_blocking(move || {
        run_blocking_watcher(&watch_root, &tx)
    })
    .await
    .context("[SYNC] Watcher task panicked")?
}

// ── Internal blocking watcher loop ──────────────────────────────────────────

/// Runs entirely on a blocking thread.  Creates the `RecommendedWatcher`,
/// starts watching, and drains debounced events until the channel closes.
fn run_blocking_watcher(
    root: &PathBuf,
    tx: &mpsc::UnboundedSender<SyncEvent>,
) -> Result<()> {
    // std channel for notify → our loop
    let (notify_tx, notify_rx) = std_mpsc::channel::<Result<Event, notify::Error>>();

    let mut watcher: RecommendedWatcher = RecommendedWatcher::new(
        move |res| {
            let _ = notify_tx.send(res);
        },
        Config::default().with_poll_interval(Duration::from_millis(DEBOUNCE_MS)),
    )
    .context("[SYNC] Failed to create file watcher")?;

    watcher
        .watch(root.as_ref(), RecursiveMode::Recursive)
        .with_context(|| {
            format!("[SYNC] Failed to start watching: {}", root.display())
        })?;

    tracing::info!("[SYNC] Watcher active — debounce {}ms", DEBOUNCE_MS);

    // Drain events with a small timeout so we naturally coalesce bursts
    // and can detect a closed `tx` in a timely manner.
    let drain_timeout = Duration::from_millis(DEBOUNCE_MS);

    // Collect events over the debounce window and de-duplicate before sending.
    let mut pending: Vec<SyncEvent> = Vec::new();

    loop {
        // Block for at most `drain_timeout` waiting for the next raw event.
        match notify_rx.recv_timeout(drain_timeout) {
            Ok(Ok(event)) => {
                if let Some(ev) = translate_event(&event, root) {
                    pending.push(ev);
                }
                // Continue draining everything available right now.
                while let Ok(Ok(event)) = notify_rx.try_recv() {
                    if let Some(ev) = translate_event(&event, root) {
                        pending.push(ev);
                    }
                }
            }
            Ok(Err(e)) => {
                tracing::warn!("[SYNC] Watcher error (continuing): {}", e);
                continue;
            }
            Err(std_mpsc::RecvTimeoutError::Timeout) => {
                // Nothing new — fall through to flush pending.
            }
            Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                tracing::info!("[SYNC] Watcher channel disconnected, stopping");
                break;
            }
        }

        // Flush pending events — deduplicate by keeping the *last* event
        // for each path (the final state after the burst matters most).
        flush_pending(&mut pending, tx);

        // If the TUI / sync server dropped the receiver, stop watching.
        if tx.is_closed() {
            tracing::info!("[SYNC] Sync event channel closed, stopping watcher");
            break;
        }
    }

    Ok(())
}

// ── Event translation ───────────────────────────────────────────────────────

/// Map a raw `notify::Event` to our domain [`SyncEvent`], returning `None`
/// for events we don't care about (access, metadata-only, etc.).
fn translate_event(event: &Event, root: &PathBuf) -> Option<SyncEvent> {
    // notify can fire events with zero paths; skip those.
    let abs_path = event.paths.first()?;

    // Make the path relative to the watched root.
    let relative = abs_path.strip_prefix(root).ok()?.to_path_buf();

    // Skip hidden / temporary files that editors tend to create.
    if is_temp_or_hidden(&relative) {
        return None;
    }

    match &event.kind {
        // ── Creates ─────────────────────────────────────────────
        EventKind::Create(CreateKind::File) => {
            Some(SyncEvent::FileCreated {
                relative_path: relative,
            })
        }
        EventKind::Create(CreateKind::Folder) => {
            Some(SyncEvent::DirCreated {
                relative_path: relative,
            })
        }
        // Some platforms emit a generic Create without sub-kind.
        EventKind::Create(CreateKind::Any) => {
            if abs_path.is_dir() {
                Some(SyncEvent::DirCreated {
                    relative_path: relative,
                })
            } else {
                Some(SyncEvent::FileCreated {
                    relative_path: relative,
                })
            }
        }

        // ── Modifications ───────────────────────────────────────
        EventKind::Modify(ModifyKind::Data(_)) | EventKind::Modify(ModifyKind::Any) => {
            // Only care about regular files, not directories.
            if !abs_path.is_dir() {
                Some(SyncEvent::FileModified {
                    relative_path: relative,
                })
            } else {
                None
            }
        }

        // ── Renames ─────────────────────────────────────────────
        // Treat rename-to as create, rename-from as delete.
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
            if abs_path.is_dir() {
                Some(SyncEvent::DirCreated {
                    relative_path: relative,
                })
            } else {
                Some(SyncEvent::FileCreated {
                    relative_path: relative,
                })
            }
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
            Some(SyncEvent::FileDeleted {
                relative_path: relative,
            })
        }

        // ── Removes ─────────────────────────────────────────────
        EventKind::Remove(RemoveKind::File)
        | EventKind::Remove(RemoveKind::Any) => {
            Some(SyncEvent::FileDeleted {
                relative_path: relative,
            })
        }
        EventKind::Remove(RemoveKind::Folder) => {
            // Folder removal — we only track file deletes for sync.
            // The listener will handle missing parents when writing.
            None
        }

        _ => None,
    }
}

/// Returns `true` for paths that are likely editor swap / temp files.
fn is_temp_or_hidden(path: &PathBuf) -> bool {
    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return true,
    };

    // Common patterns: .git, .DS_Store, vim swap, ~ backup
    name.starts_with('.')
        || name.ends_with('~')
        || name.ends_with(".swp")
        || name.ends_with(".swx")
        || name.ends_with(".tmp")
        || name.starts_with("~$") // Office lock files
}

// ── De-duplication flush ────────────────────────────────────────────────────

/// Send all pending events through `tx`, de-duplicating so that only the
/// **last** event per path is emitted (final-state-wins within a burst).
fn flush_pending(
    pending: &mut Vec<SyncEvent>,
    tx: &mpsc::UnboundedSender<SyncEvent>,
) {
    if pending.is_empty() {
        return;
    }

    // Walk backwards; the first occurrence (from the back) for a given
    // path is the one we keep.
    let mut seen = std::collections::HashSet::<PathBuf>::new();
    let mut deduped: Vec<SyncEvent> = Vec::with_capacity(pending.len());

    for ev in pending.drain(..).rev() {
        let path = event_path(&ev).clone();
        if seen.insert(path) {
            deduped.push(ev);
        }
    }

    // Reverse so events are sent in chronological order.
    deduped.reverse();

    for ev in deduped {
        tracing::debug!("[SYNC] Event: {:?}", ev);
        if tx.send(ev).is_err() {
            // Receiver dropped — caller will notice via `is_closed()`.
            return;
        }
    }
}

/// Extract the relative path from any [`SyncEvent`] variant.
fn event_path(ev: &SyncEvent) -> &PathBuf {
    match ev {
        SyncEvent::FileCreated { relative_path }
        | SyncEvent::FileModified { relative_path }
        | SyncEvent::FileDeleted { relative_path }
        | SyncEvent::DirCreated { relative_path } => relative_path,
    }
}
