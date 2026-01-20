class AddCompletedToAssignments < ActiveRecord::Migration[7.2]
  def change
    add_column :assignments, :completed, :boolean, default: false
    add_column :assignments, :completed_at, :datetime
  end
end
