class AddIsFrozenToCourses < ActiveRecord::Migration[7.2]
  def change
    add_column :courses, :is_frozen, :boolean, default: false, null: false unless column_exists?(:courses, :is_frozen)
  end
end
