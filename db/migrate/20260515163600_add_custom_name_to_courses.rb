class AddCustomNameToCourses < ActiveRecord::Migration[7.2]
  def change
    add_column :courses, :custom_name, :string
  end
end
