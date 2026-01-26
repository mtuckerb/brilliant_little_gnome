class AddShowRecentUpdatesToUserPreferences < ActiveRecord::Migration[7.2]
  def change
    add_column :user_preferences, :show_recent_updates, :boolean, default: true
  end
end
