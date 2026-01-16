class AddApiSettingsToUserPreferences < ActiveRecord::Migration[5.2]
  def change
    add_column :user_preferences, :api_key, :string
    add_column :user_preferences, :api_enabled, :boolean, default: false
    add_column :user_preferences, :api_listen_all, :boolean, default: false
  end
end
