
require_relative 'app'
ActiveRecord::MigrationContext.new("db/migrate").migrate
puts "Migrations complete."
