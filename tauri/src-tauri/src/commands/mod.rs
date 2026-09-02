pub mod auth;
pub mod diagnostics;
pub mod prefs;
pub mod courses;
pub mod grades;
pub mod assignments;
pub mod notifications;
pub mod sync;
pub mod rest;
pub mod content;
pub mod content_cache;
pub mod discussions;
pub mod overview;
pub mod assignment_detail;
pub mod downloads;
pub mod htmlmd;
pub mod updates;
pub mod zotero;
pub mod import_old;

#[cfg(feature = "p2p")]
pub mod sync_p2p;

use crate::error::Result;
use crate::state::AppState;
use std::sync::Arc;
use tauri::State;

pub type AppStateArg<'a> = State<'a, Arc<AppState>>;

pub fn ok<T>(v: T) -> Result<T> { Ok(v) }
