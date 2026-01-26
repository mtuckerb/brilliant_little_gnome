class AddRemoteServerAndJwtToUserPreferences < ActiveRecord::Migration[7.1]
  def change
    add_column :user_preferences, :remote_server_enabled, :boolean, default: false
    add_column :user_preferences, :remote_server_ip, :string
    add_column :user_preferences, :remote_server_port, :integer, default: 4567
    add_column :user_preferences, :jwt_secret, :string
  end
end
