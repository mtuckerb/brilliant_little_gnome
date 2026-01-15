class AddIsReadToNotifications < ActiveRecord::Migration[5.2]
  def change
    add_column :notifications, :is_read, :boolean, default: false, index: true
  end
end
