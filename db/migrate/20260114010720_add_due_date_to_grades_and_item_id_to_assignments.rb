class AddDueDateToGradesAndItemIdToAssignments < ActiveRecord::Migration[5.2]
  def change
    add_column :grades, :due_date, :datetime
    add_column :assignments, :grade_item_id, :string
    add_index :assignments, :grade_item_id
  end
end
