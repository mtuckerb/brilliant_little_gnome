class CreateUserPreferences < ActiveRecord::Migration[5.2]
  def change
    create_table :user_preferences do |t|
      t.string :display_name
      t.string :time_zone, default: 'UTC'
      t.string :brightspace_host
      t.string :brightspace_cookie
      t.timestamps
    end
  end
end
