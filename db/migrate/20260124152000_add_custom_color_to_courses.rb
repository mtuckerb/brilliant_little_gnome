class AddCustomColorToCourses < ActiveRecord::Migration[7.0]
  def change
    add_column :courses, :custom_color, :string unless column_exists?(:courses, :custom_color)
  end
end
