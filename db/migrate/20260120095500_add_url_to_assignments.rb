class AddUrlToAssignments < ActiveRecord::Migration[5.2]
  def change
    add_column :assignments, :external_url, :string
  end
end
