# This file is auto-generated from the current state of the database. Instead
# of editing this file, please use the migrations feature of Active Record to
# incrementally modify your database, and then regenerate this schema definition.
#
# This file is the source Rails uses to define your schema when running `bin/rails
# db:schema:load`. When creating a new database, `bin/rails db:schema:load` tends to
# be faster and is potentially less error prone than running all of your
# migrations from scratch. Old migrations may fail to apply correctly if those
# migrations use external dependencies or application code.
#
# It's strongly recommended that you check this file into your version control system.

ActiveRecord::Schema[7.2].define(version: 2026_01_19_170000) do
  create_table "api_caches", force: :cascade do |t|
    t.string "path"
    t.text "data"
    t.datetime "created_at", precision: nil, null: false
    t.datetime "updated_at", precision: nil, null: false
    t.boolean "is_archived", default: false
    t.index ["path"], name: "index_api_caches_on_path"
  end

  create_table "assignments", force: :cascade do |t|
    t.string "course_id"
    t.string "brightspace_id"
    t.string "name"
    t.datetime "due_date", precision: nil
    t.text "description"
    t.boolean "is_graded", default: false
    t.datetime "created_at", precision: nil, null: false
    t.datetime "updated_at", precision: nil, null: false
    t.string "grade_item_id"
    t.string "assignment_type", default: "dropbox"
    t.boolean "completed", default: false
    t.datetime "completed_at"
    t.index ["brightspace_id"], name: "index_assignments_on_brightspace_id"
    t.index ["course_id"], name: "index_assignments_on_course_id"
    t.index ["grade_item_id"], name: "index_assignments_on_grade_item_id"
  end

  create_table "content_items", force: :cascade do |t|
    t.string "module_id"
    t.string "brightspace_id"
    t.string "title"
    t.string "item_type"
    t.string "url"
    t.boolean "is_hidden", default: false
    t.datetime "created_at", precision: nil, null: false
    t.datetime "updated_at", precision: nil, null: false
    t.integer "sort_order"
    t.index ["brightspace_id"], name: "index_content_items_on_brightspace_id"
    t.index ["module_id"], name: "index_content_items_on_module_id"
  end

  create_table "content_modules", force: :cascade do |t|
    t.string "course_id"
    t.string "brightspace_id"
    t.string "title"
    t.text "description"
    t.integer "sort_order"
    t.string "parent_id"
    t.datetime "created_at", precision: nil, null: false
    t.datetime "updated_at", precision: nil, null: false
    t.index ["brightspace_id"], name: "index_content_modules_on_brightspace_id"
    t.index ["course_id"], name: "index_content_modules_on_course_id"
    t.index ["parent_id"], name: "index_content_modules_on_parent_id"
  end

  create_table "courses", force: :cascade do |t|
    t.string "org_unit_id"
    t.string "name"
    t.string "code"
    t.string "semester"
    t.datetime "last_accessed_at", precision: nil
    t.boolean "is_pinned", default: false
    t.datetime "created_at", precision: nil, null: false
    t.datetime "updated_at", precision: nil, null: false
    t.integer "units", default: 3
    t.float "target_grade", default: 93.0
    t.string "banner_url"
    t.index ["org_unit_id"], name: "index_courses_on_org_unit_id"
  end

  create_table "discussion_forums", force: :cascade do |t|
    t.string "brightspace_id"
    t.string "course_id"
    t.string "name"
    t.text "description"
    t.datetime "created_at", precision: nil, null: false
    t.datetime "updated_at", precision: nil, null: false
    t.index ["brightspace_id"], name: "index_discussion_forums_on_brightspace_id"
    t.index ["course_id"], name: "index_discussion_forums_on_course_id"
  end

  create_table "discussion_posts", force: :cascade do |t|
    t.string "brightspace_id"
    t.string "topic_id"
    t.string "thread_id"
    t.string "parent_post_id"
    t.string "subject"
    t.text "body"
    t.string "author_name"
    t.datetime "posted_at", precision: nil
    t.boolean "is_instructor", default: false
    t.datetime "created_at", precision: nil, null: false
    t.datetime "updated_at", precision: nil, null: false
    t.index ["brightspace_id"], name: "index_discussion_posts_on_brightspace_id"
    t.index ["parent_post_id"], name: "index_discussion_posts_on_parent_post_id"
    t.index ["thread_id"], name: "index_discussion_posts_on_thread_id"
    t.index ["topic_id"], name: "index_discussion_posts_on_topic_id"
  end

  create_table "discussion_threads", force: :cascade do |t|
    t.string "brightspace_id"
    t.string "course_id"
    t.string "topic_id"
    t.string "subject"
    t.text "body"
    t.string "author_name"
    t.datetime "posted_at", precision: nil
    t.boolean "is_pinned", default: false
    t.integer "unread_count", default: 0
    t.datetime "created_at", precision: nil, null: false
    t.datetime "updated_at", precision: nil, null: false
    t.index ["brightspace_id"], name: "index_discussion_threads_on_brightspace_id"
    t.index ["course_id"], name: "index_discussion_threads_on_course_id"
    t.index ["topic_id"], name: "index_discussion_threads_on_topic_id"
  end

  create_table "discussion_topics", force: :cascade do |t|
    t.string "brightspace_id"
    t.string "course_id"
    t.string "forum_id"
    t.string "name"
    t.text "description"
    t.integer "sort_order"
    t.integer "thread_count", default: 0
    t.integer "post_count", default: 0
    t.datetime "last_post_date", precision: nil
    t.datetime "created_at", precision: nil, null: false
    t.datetime "updated_at", precision: nil, null: false
    t.index ["brightspace_id"], name: "index_discussion_topics_on_brightspace_id"
    t.index ["course_id"], name: "index_discussion_topics_on_course_id"
    t.index ["forum_id"], name: "index_discussion_topics_on_forum_id"
  end

  create_table "grades", force: :cascade do |t|
    t.string "course_id"
    t.string "brightspace_id"
    t.string "name"
    t.string "displayed_grade"
    t.float "numerator"
    t.float "denominator"
    t.string "grade_object_type"
    t.datetime "last_modified", precision: nil
    t.text "comments"
    t.datetime "created_at", precision: nil, null: false
    t.datetime "updated_at", precision: nil, null: false
    t.float "weight"
    t.datetime "due_date", precision: nil
    t.index ["brightspace_id"], name: "index_grades_on_brightspace_id"
    t.index ["course_id", "brightspace_id"], name: "index_grades_on_course_id_and_brightspace_id", unique: true
    t.index ["course_id"], name: "index_grades_on_course_id"
  end

  create_table "notifications", force: :cascade do |t|
    t.string "external_id"
    t.string "notification_type"
    t.string "title"
    t.text "body"
    t.datetime "date", precision: nil
    t.string "course_id"
    t.string "course_name"
    t.string "semester"
    t.integer "urgency"
    t.boolean "is_personal"
    t.string "url"
    t.datetime "created_at", precision: nil, null: false
    t.datetime "updated_at", precision: nil, null: false
    t.boolean "is_read", default: false
    t.index ["course_id"], name: "index_notifications_on_course_id"
    t.index ["external_id", "course_id"], name: "index_notifications_on_external_id_and_course_id", unique: true
    t.index ["external_id"], name: "index_notifications_on_external_id"
  end

  create_table "user_preferences", force: :cascade do |t|
    t.string "display_name"
    t.string "time_zone", default: "UTC"
    t.string "brightspace_host"
    t.string "brightspace_cookie"
    t.datetime "created_at", precision: nil, null: false
    t.datetime "updated_at", precision: nil, null: false
    t.text "collapsed_topics"
    t.float "historic_gpa"
    t.integer "historic_units"
    t.string "default_semester"
    t.string "last_notification_sync_at"
    t.string "force_full_sync", default: "false"
    t.string "api_key"
    t.boolean "api_enabled", default: false
    t.boolean "api_listen_all", default: false
  end
end
