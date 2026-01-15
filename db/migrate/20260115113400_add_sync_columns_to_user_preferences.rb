class AddSyncColumnsToUserPreferences < ActiveRecord::Migration[5.2]
  def change
    add_column :user_preferences, :last_notification_sync_at, :string
    add_column :user_preferences, :force_full_sync, :string, default: 'false'
  end
end
