class AddGpaTrackingColumns < ActiveRecord::Migration[5.2]
  def change
    add_column :courses, :units, :integer, default: 3
    add_column :user_preferences, :historic_gpa, :float
    add_column :user_preferences, :historic_units, :integer
  end
end
