class AddSortOrderToCourses < ActiveRecord::Migration[7.2]
  def change
    add_column :courses, :sort_order, :integer, default: 0
  end
end
