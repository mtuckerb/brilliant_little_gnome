class AddAssignmentTypeToAssignments < ActiveRecord::Migration[7.1]
  def change
    add_column :assignments, :assignment_type, :string, default: 'dropbox'
  end
end
