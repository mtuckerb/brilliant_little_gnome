#!/bin/bash
DB="db/development.sqlite3"

echo "Updating user_preferences..."
sqlite3 $DB "ALTER TABLE user_preferences ADD COLUMN brightspace_uid VARCHAR;"
sqlite3 $DB "ALTER TABLE user_preferences ADD COLUMN brightspace_user_id INTEGER;"
sqlite3 $DB "ALTER TABLE user_preferences ADD COLUMN last_login_at DATETIME;"

TABLES=("courses" "content_modules" "content_items" "assignments" "grades" "discussion_forums" "discussion_topics" "discussion_threads" "discussion_posts" "notifications" "api_caches")

for table in "${TABLES[@]}"; do
    echo "Updating $table..."
    sqlite3 $DB "ALTER TABLE $table ADD COLUMN user_id VARCHAR;"
    sqlite3 $DB "CREATE INDEX index_${table}_on_user_id ON $table (user_id);"
done

echo "Migration complete."
