class AddManuallyEditedToAssignments < ActiveRecord::Migration[7.2]
  def change
    add_column :assignments, :manually_edited, :boolean, default: false
    add_column :assignments, :manually_edited_at, :datetime
  end
end
