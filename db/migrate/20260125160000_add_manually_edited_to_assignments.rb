class AddManuallyEditedToAssignments < ActiveRecord::Migration[7.2]
  def change
    add_column :assignments, :manually_edited, :boolean, default: false unless column_exists?(:assignments, :manually_edited)
    add_column :assignments, :manually_edited_at, :datetime unless column_exists?(:assignments, :manually_edited_at)
  end
end
