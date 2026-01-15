class AddCollapsedTopicsToUserPreferences < ActiveRecord::Migration[5.2]
  def change
    add_column :user_preferences, :collapsed_topics, :text
  end
end
