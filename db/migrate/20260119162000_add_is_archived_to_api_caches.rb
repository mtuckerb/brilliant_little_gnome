class AddIsArchivedToApiCaches < ActiveRecord::Migration[5.2]
  def change
    add_column :api_caches, :is_archived, :boolean, default: false
  end
end
