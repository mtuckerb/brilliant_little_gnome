class AddDefaultSemesterToUserPreferences < ActiveRecord::Migration[5.2]
  def change
    add_column :user_preferences, :default_semester, :string
  end
end
