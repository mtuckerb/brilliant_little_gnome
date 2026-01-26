class EnablePolyglotIdentity < ActiveRecord::Migration[7.2]
  def change
    # 1. Update User Preferences for Identity
    add_column :user_preferences, :brightspace_uid, :string unless column_exists?(:user_preferences, :brightspace_uid)
    add_column :user_preferences, :brightspace_user_id, :integer unless column_exists?(:user_preferences, :brightspace_user_id)
    add_column :user_preferences, :last_login_at, :datetime unless column_exists?(:user_preferences, :last_login_at)

    # 2. Add user_id to high-level content tables
    tables = [
      :courses,
      :content_modules,
      :content_items,
      :assignments,
      :grades,
      :discussion_forums,
      :discussion_topics,
      :discussion_threads,
      :discussion_posts,
      :notifications,
      :api_caches
    ]

    tables.each do |table_name|
      add_column table_name, :user_id, :string unless column_exists?(table_name, :user_id)
      add_index table_name, :user_id unless index_exists?(table_name, :user_id)
    end
  end
end
