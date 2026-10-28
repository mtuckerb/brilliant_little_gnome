-- Device-local, like Zotero preferences. Keep Brightspace source URLs intact
-- so later course syncs cannot overwrite a successful Zotero destination.
CREATE TABLE zotero_document_links (
    library_id TEXT NOT NULL,
    course_id TEXT NOT NULL,
    source_id TEXT NOT NULL, -- Brightspace topic ID, or 'syllabus'
    url TEXT NOT NULL,
    PRIMARY KEY (library_id, course_id, source_id)
);
