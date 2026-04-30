// Loro CRDT wrapper. Schema layout in design.md §4 + field map §10.
//
// The actual `loro::LoroDoc` and typed accessors land in T-004, after
// we've sat with loro 1.12's Map/Text API. Today this is just the
// type that other modules can name.

#![allow(dead_code)]

/// Wrapper around a `loro::LoroDoc` exposing typed getters/setters
/// mirroring the field map in design.md §10. Filled in by T-004.
pub struct SyncDoc {
    // T-004: holds `loro::LoroDoc` plus cached container handles
    // (root Map, prefs Map, course_overlays Map, etc.) so each
    // setter doesn't re-resolve paths from the root.
}
