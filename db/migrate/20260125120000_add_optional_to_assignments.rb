class AddOptionalToAssignments < ActiveRecord::Migration[7.2]
  def change
    add_column :assignments, :optional, :boolean, default: false
  end
end
