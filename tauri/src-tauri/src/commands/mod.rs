pub mod auth;
pub mod prefs;
pub mod courses;
pub mod grades;
pub mod assignments;
pub mod notifications;
pub mod sync;
pub mod rest;
pub mod content;
pub mod discussions;

use crate::error::Result;
use crate::state::AppState;
use std::sync::Arc;
use tauri::State;

pub type AppStateArg<'a> = State<'a, Arc<AppState>>;

pub fn ok<T>(v: T) -> Result<T> { Ok(v) }
