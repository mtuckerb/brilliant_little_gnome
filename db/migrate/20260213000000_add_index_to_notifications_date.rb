class AddIndexToNotificationsDate < ActiveRecord::Migration[5.2]
  def change
    add_index :notifications, :date
    add_index :notifications, :urgency
    add_index :notifications, :is_read
  end
end
