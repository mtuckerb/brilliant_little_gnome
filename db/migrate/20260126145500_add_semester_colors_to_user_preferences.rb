class AddSemesterColorsToUserPreferences < ActiveRecord::Migration[7.2]
  def change
    add_column :user_preferences, :semester_colors, :text
  end
end
