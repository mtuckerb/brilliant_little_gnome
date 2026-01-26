class AddOptionalToAssignments < ActiveRecord::Migration[7.2]
  def change
    add_column :assignments, :optional, :boolean, default: false unless column_exists?(:assignments, :optional)
  end
end
