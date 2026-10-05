# iOS termination callbacks

This is the published `tao` 0.35.3 crate, with its Apache-2.0 license retained.
Only `src/platform_impl/ios/app_state.rs` differs from the published source.

UIKit can dispatch final layout events after `LoopDestroyed`, particularly when
an iOS app runs on an Apple Silicon Mac. Upstream treats these as a bug and aborts:
[tao #1314](https://github.com/tauri-apps/tao/issues/1314).

The patch returns a terminal callback result, which both user and non-user event
dispatchers discard. Run-loop callbacks and GPU redraws also ignore terminated
state, the poll timer stops at termination, and duplicate termination is harmless.
The remaining lifecycle invariants are unchanged. Native iOS unit tests exercise
late callbacks, run-loop callbacks, and normal active callback transitions.

Tauri 2.11.5 / runtime-wry 2.11.4 separately reloads a WKWebView whose web content
process was killed, fixing the blank screen on resume:
[tauri #14523](https://github.com/tauri-apps/tauri/pull/14523).

Remove the Cargo patch and this directory once the Tao release selected by Tauri
handles post-termination callbacks; retain and rerun the lifecycle regressions.
