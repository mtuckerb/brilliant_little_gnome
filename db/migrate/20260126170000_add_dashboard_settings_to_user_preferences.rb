class AddDashboardSettingsToUserPreferences < ActiveRecord::Migration[7.2]
  def change
    add_column :user_preferences, :show_upcoming_assignments, :boolean, default: true
    add_column :user_preferences, :show_course_list, :boolean, default: true
  end
end
