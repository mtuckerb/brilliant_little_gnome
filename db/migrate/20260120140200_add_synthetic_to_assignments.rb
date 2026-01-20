class AddSyntheticToAssignments < ActiveRecord::Migration[5.2]
  def change
    add_column :assignments, :synthetic, :boolean, default: false
  end
end
