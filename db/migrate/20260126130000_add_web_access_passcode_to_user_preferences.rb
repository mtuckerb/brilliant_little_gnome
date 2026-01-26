class AddWebAccessPasscodeToUserPreferences < ActiveRecord::Migration[7.1]
  def change
    unless column_exists?(:user_preferences, :web_access_passcode)
      add_column :user_preferences, :web_access_passcode, :string
    end
  end
end
