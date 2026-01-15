class AddSortOrderToContentItems < ActiveRecord::Migration[5.2]
  def change
    add_column :content_items, :sort_order, :integer
  end
end
